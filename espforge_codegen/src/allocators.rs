use espforge_configuration::EspforgeConfiguration;
use proc_macro2::TokenStream;
use quote::quote;

/// Generates allocator initialization code for heap and PSRAM
pub struct AllocatorGenerator;

impl AllocatorGenerator {
    /// Generate both heap and PSRAM allocator code based on model configuration
    pub fn generate(model: &EspforgeConfiguration) -> TokenStream {
        let heap = Self::heap_allocator(model);
        let psram = Self::psram_allocator(model);

        quote! {
            #psram
            #heap
        }
    }

    /// Generate heap allocator if configured
    fn heap_allocator(model: &EspforgeConfiguration) -> TokenStream {
        model
            .get_heap_size()
            .map(|size| {
                quote! {
                    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: #size);
                }
            })
            .unwrap_or_else(TokenStream::new)
    }

    /// Generate PSRAM allocator if PSRAM is enabled
    fn psram_allocator(model: &EspforgeConfiguration) -> TokenStream {
        if model.has_psram() == Some(true) {
            quote! {
                esp_alloc::psram_allocator!(unsafe { esp_hal::peripherals::PSRAM::steal() }, esp_hal::psram);
            }
        } else {
            TokenStream::new()
        }
    }
}
