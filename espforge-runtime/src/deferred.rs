//! Deferred device construction: build the handle at boot, initialise the part
//! after `app::setup` has had a chance to power it.
//!
//! `Devices::new` runs before the executor starts and before the app can
//! enable a power rail, so a device that must be brought up later stores a
//! *factory* instead of its driver. The field is a [`Deferred`] (or an
//! [`AsyncDeferred`]), never an `Option`: the slot's state is private, and a
//! caller either gets the device or a typed reason.
//!
//! A factory that fails is consumed; a second `init()` reports the failure
//! instead of retrying. Retrying would need a construction the driver cannot
//! repeat (the endpoint's pins were moved into the first attempt).

/// The device is not initialised yet. [`Deferred::get_mut`] answers this
/// instead of panicking, so an app can check without an `Option` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotReady;

impl core::fmt::Display for NotReady {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("device is not initialized yet; call `init()` first")
    }
}

/// Builds a device after boot. Generated once per deferred device, holding the
/// bus endpoint and the pins the constructor needs.
pub trait DeferredInit {
    /// The driver type the app receives once the part answers.
    type Device;
    /// Run the constructor and its I/O. Called at most once per device.
    fn init(self) -> Result<Self::Device, crate::DeviceError>;
}

/// A device built at boot but initialised on demand.
pub struct Deferred<F: DeferredInit> {
    ready: Option<F::Device>,
    factory: Option<F>,
}

impl<F: DeferredInit> Deferred<F> {
    /// Store the factory. No bus traffic and no device init happens here.
    pub fn new(factory: F) -> Self {
        Self {
            ready: None,
            factory: Some(factory),
        }
    }

    /// Whether [`init`](Self::init) has succeeded.
    pub fn is_ready(&self) -> bool {
        self.ready.is_some()
    }

    /// Run the factory once; later calls return the same device.
    ///
    /// The error carries the element id the factory named, so the app's log
    /// says which part did not answer.
    pub fn init(&mut self) -> Result<&mut F::Device, crate::DeviceError> {
        if self.ready.is_none()
            && let Some(factory) = self.factory.take()
        {
            self.ready = Some(factory.init()?);
        }
        self.ready.as_mut().ok_or(crate::DeviceError {
            device: "deferred",
            kind: crate::DeviceErrorKind::Init,
        })
    }

    /// The device, or [`NotReady`] before a successful [`init`](Self::init).
    pub fn get_mut(&mut self) -> Result<&mut F::Device, NotReady> {
        self.ready.as_mut().ok_or(NotReady)
    }
}

/// The async counterpart of [`DeferredInit`].
///
/// The future is not required to be `Send`: esp-hal's async bus handles carry
/// a `PhantomData<*const ()>` and are deliberately `!Send`, so a bound here
/// would reject every real panel. The generated entry point awaits `init`
/// before any task is spawned, which is where an embassy executor needs it.
#[cfg(feature = "embassy")]
pub trait AsyncDeferredInit {
    /// The driver type the app receives once the part answers.
    type Device;
    /// Run the constructor and its I/O. Called at most once per device.
    fn init(self) -> impl core::future::Future<Output = Result<Self::Device, crate::DeviceError>>;
}

/// A device initialised on demand from an async constructor.
///
/// An async device is always deferred: `Devices::new` is synchronous and runs
/// before the executor, so there is no place to await a constructor at boot.
#[cfg(feature = "embassy")]
pub struct AsyncDeferred<F: AsyncDeferredInit> {
    ready: Option<F::Device>,
    factory: Option<F>,
}

#[cfg(feature = "embassy")]
impl<F: AsyncDeferredInit> AsyncDeferred<F> {
    /// Store the factory. No bus traffic and no device init happens here.
    pub fn new(factory: F) -> Self {
        Self {
            ready: None,
            factory: Some(factory),
        }
    }

    /// Whether [`init`](Self::init) has succeeded.
    pub fn is_ready(&self) -> bool {
        self.ready.is_some()
    }

    /// Run the factory once; later calls return the same device.
    pub async fn init(&mut self) -> Result<&mut F::Device, crate::DeviceError> {
        if self.ready.is_none()
            && let Some(factory) = self.factory.take()
        {
            self.ready = Some(factory.init().await?);
        }
        self.ready.as_mut().ok_or(crate::DeviceError {
            device: "deferred",
            kind: crate::DeviceErrorKind::Init,
        })
    }

    /// The device, or [`NotReady`] before a successful [`init`](Self::init).
    pub fn get_mut(&mut self) -> Result<&mut F::Device, NotReady> {
        self.ready.as_mut().ok_or(NotReady)
    }
}

/// The mutex an async device is stored in.
///
/// A re-export, not a new dependency: `espforge-runtime` pins
/// `embassy-sync` 0.7 while a generated project may name another version, and
/// two versions of the same type cannot meet at a field boundary. Generated
/// code therefore names only this alias.
#[cfg(feature = "embassy")]
pub type AsyncMutex<T> =
    embassy_sync::mutex::Mutex<embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex, T>;

/// The guard [`AsyncMutex::lock`] hands out.
#[cfg(feature = "embassy")]
pub type AsyncMutexGuard<'a, T> = embassy_sync::mutex::MutexGuard<
    'a,
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    T,
>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceError, DeviceErrorKind};
    use std::cell::Cell;
    use std::rc::Rc;

    struct Counter {
        calls: Rc<Cell<u32>>,
        result: Result<u32, DeviceErrorKind>,
    }

    impl DeferredInit for Counter {
        type Device = u32;

        fn init(self) -> Result<u32, DeviceError> {
            self.calls.set(self.calls.get() + 1);
            self.result.map_err(|kind| DeviceError {
                device: "counter",
                kind,
            })
        }
    }

    #[test]
    fn construction_waits_for_init_and_only_runs_once() {
        let calls = Rc::new(Cell::new(0));
        let mut slot = Deferred::new(Counter {
            calls: calls.clone(),
            result: Ok(7),
        });

        assert_eq!(calls.get(), 0, "`new` performs no construction");
        assert_eq!(slot.get_mut(), Err(NotReady));
        assert_eq!(*slot.init().unwrap(), 7);
        assert_eq!(calls.get(), 1);
        assert_eq!(*slot.init().unwrap(), 7);
        assert_eq!(calls.get(), 1, "a second init reuses the slot");
        assert_eq!(*slot.get_mut().unwrap(), 7);
        assert!(slot.is_ready());
    }

    #[test]
    fn a_failed_factory_leaves_the_slot_not_ready() {
        let calls = Rc::new(Cell::new(0));
        let mut slot = Deferred::new(Counter {
            calls: calls.clone(),
            result: Err(DeviceErrorKind::Init),
        });

        let error = slot.init().unwrap_err();
        assert_eq!(error.device, "counter");
        assert_eq!(error.kind, DeviceErrorKind::Init);
        assert_eq!(slot.get_mut(), Err(NotReady));
        assert!(!slot.is_ready());
        // The factory was consumed: a retry reports instead of panicking.
        assert!(slot.init().is_err());
        assert_eq!(calls.get(), 1);
    }

    /// Two tasks contend on one device: the second waits for the guard instead
    /// of panicking. Polled by hand, because the host has no embassy executor.
    #[cfg(feature = "embassy")]
    #[test]
    fn two_tasks_contend_on_one_device_without_panicking() {
        use core::future::Future;
        use core::pin::pin;
        use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

        /// Returns `Pending` once, so the first task holds its guard across a
        /// suspension point while the second one tries to lock.
        struct YieldOnce(bool);
        impl Future for YieldOnce {
            type Output = ();
            fn poll(mut self: core::pin::Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
                if self.0 {
                    Poll::Ready(())
                } else {
                    self.0 = true;
                    Poll::Pending
                }
            }
        }

        fn noop_waker() -> Waker {
            const VTABLE: RawWakerVTable = RawWakerVTable::new(|_| RAW, |_| {}, |_| {}, |_| {});
            const RAW: RawWaker = RawWaker::new(core::ptr::null(), &VTABLE);
            // SAFETY: the vtable's operations do nothing and never dereference
            // the data pointer.
            unsafe { Waker::from_raw(RAW) }
        }

        let mutex = AsyncMutex::new(0u32);
        let mut first = pin!(async {
            let mut guard = mutex.lock().await;
            *guard += 1;
            YieldOnce(false).await;
            *guard += 1;
            *guard
        });
        let mut second = pin!(async {
            let mut guard = mutex.lock().await;
            *guard += 10;
            *guard
        });

        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        assert_eq!(first.as_mut().poll(&mut cx), Poll::Pending);
        assert_eq!(
            second.as_mut().poll(&mut cx),
            Poll::Pending,
            "the second task queues instead of taking a second borrow"
        );
        assert_eq!(first.as_mut().poll(&mut cx), Poll::Ready(2));
        assert_eq!(second.as_mut().poll(&mut cx), Poll::Ready(12));
        assert_eq!(*mutex.try_lock().unwrap(), 12);
    }
}
