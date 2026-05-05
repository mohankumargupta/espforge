use espforge_configuration::EspforgeConfiguration;
use proc_macro2::TokenStream;
use quote::quote;

/// Generates the two background tasks (`wifi_connection_task` and `net_task`)
/// that must be emitted at module level in `main.rs` when wifi is present.
pub fn generate_wifi_tasks(model: &EspforgeConfiguration) -> TokenStream {
    if model.esp32.as_ref().and_then(|e| e.wifi.as_ref()).is_none() {
        return quote! {};
    }

    quote! {
        #[embassy_executor::task]
        async fn wifi_connection_task(
            mut controller: esp_radio::wifi::WifiController<'static>,
        ) {
            //controller.start_async().await.unwrap();

            esp_println::println!("WiFi controller started");

            let scan_config = esp_radio::wifi::scan::ScanConfig::default().with_max(10);
            let result = controller.scan_async(&scan_config).await.unwrap();
            for ap in result {
                esp_println::println!("{:?}", ap);
            }

            loop {
                esp_println::println!("Attempting to connect to WiFi...");
                match controller.connect_async().await {
                    Ok(_info) => {
                       esp_println::println!("WiFi connected successfully!");
                       let _ = controller.wait_for_disconnect_async().await;
                       esp_println::println!("WiFi disconnected!");
                    }
                    Err(e) => {
                       esp_println::println!("WiFi connection failed: {:?}", e);
                    }
                }
                embassy_time::Timer::after(
                    embassy_time::Duration::from_millis(5000)
                ).await;
            }
        }

        #[embassy_executor::task]
        async fn net_task(
            mut runner: espforge_platform::embassy_net::Runner<'static, esp_radio::wifi::Interface<'static>>,
        ) {
            runner.run().await
        }
    }
}

/// Generates the wifi initialisation block that must be emitted **inside**
/// `async fn main()` before the `Context` is constructed.
///
/// Returns a `TokenStream` containing:
///   - peripheral setup (controller, interfaces, stack)
///   - task spawning
///   - `stack.wait_config_up()`
///   - `let wifi = WifiClient::new(…)`
pub fn generate_wifi_init(model: &EspforgeConfiguration) -> TokenStream {
    let wifi_cfg = match model.esp32.as_ref().and_then(|e| e.wifi.as_ref()) {
        Some(cfg) => cfg,
        None => return quote! {},
    };

    //let ssid = &wifi_cfg.ssid;
    //let password = &wifi_cfg.password;

    // let auth_code = match wifi_cfg.auth {
    //     espforge_configuration::hardware::wifi::AuthMode::Open => {
    //         //quote! { esp_radio::wifi::AuthMethod::None }
    //         quote! { }
    //     }
    //     espforge_configuration::hardware::wifi::AuthMode::Wpa2 => {
    //         //quote! { esp_radio::wifi::AuthMethod::Wpa2Personal }
    //         quote! { }
    //     }
    // };

    let password_auth_code = match wifi_cfg.auth {
        espforge_configuration::hardware::wifi::AuthMode::Open => quote! {
            let station_config = station_config.with_auth_method(esp_radio::wifi::AuthenticationMethod::None);
        },
        espforge_configuration::hardware::wifi::AuthMode::Wpa2 => quote! {
           let station_config = station_config.with_password(password);
        },
    };

    let password_code = match wifi_cfg.auth {
        espforge_configuration::hardware::wifi::AuthMode::Open => quote! {},
        espforge_configuration::hardware::wifi::AuthMode::Wpa2 => quote! {
            let password = env!("WIFI_PASSWORD");
        },
    };

    quote! {
        static STACK_RESOURCES: static_cell::StaticCell<espforge_platform::embassy_net::StackResources<3>> =
            static_cell::StaticCell::new();


        // static RADIO: static_cell::StaticCell<esp_radio::Controller<'static>> =
        //     static_cell::StaticCell::new();

        // let radio_ctrl = RADIO.init(esp_radio::init().unwrap());

        let rng = esp_hal::rng::Rng::new();
        let seed = (rng.random() as u64) << 32 | rng.random() as u64;

        let ssid = env!("WIFI_SSID");
        #password_code

        // let mode =  esp_radio::wifi::ModeConfig::Client(
        //                 esp_radio::wifi::ClientConfig::default()
        //                 .with_ssid(ssid.into())
        //                 .with_auth_method(#auth_code)
        // );
        let station_config = esp_radio::wifi::sta::StationConfig::default()
            .with_ssid(ssid);
        #password_auth_code
        let (mut controller, interfaces) = esp_radio::wifi::new(
            //&*radio_ctrl,
            registry.wifi.borrow_mut().take().unwrap(),
                        esp_radio::wifi::ControllerConfig::default()
                .with_initial_config(esp_radio::wifi::Config::Station(station_config))
            //esp_radio::wifi::Config::default()
        ).unwrap();
        //controller.set_config(&mode).unwrap();

        let stack_resources: &'static mut _ =
            STACK_RESOURCES.init(espforge_platform::embassy_net::StackResources::new());

        let (stack, net_runner) = espforge_platform::embassy_net::new(
            interfaces.station,
            espforge_platform::embassy_net::Config::dhcpv4(Default::default()),
            stack_resources,
            seed,
        );

        // spawner.spawn(wifi_connection_task(controller)).ok();
        // spawner.spawn(net_task(net_runner)).ok();

        spawner.spawn(wifi_connection_task(controller).unwrap());
        spawner.spawn(net_task(net_runner).unwrap());

        stack.wait_config_up().await;
    }
}
