//! Dual-slot A/B OTA firmware updates, driven from `app.rs`.
//!
//! [`Ota`] is the handle a project with `<ota />` reaches as `ctx.ota`. It
//! wraps `esp-hal-ota`, which discovers the partition table from flash at boot
//! and streams a new image into the inactive slot. The updater itself lives in
//! an [`OtaStorage`] the generated `src/generated.rs` declares, so this crate
//! never hides storage in a `static` (see the `no_static_storage_in_runtime`
//! test): the generated `main.rs` fills that cell once from the `FLASH`
//! peripheral.
//!
//! Every operation is fallible and returns [`OtaError`]; a failed download is a
//! logged, named failure the app decides about, not a panic.

use core::cell::RefCell;

use critical_section::Mutex;
use esp_hal_ota::Ota as Updater;
use esp_storage::FlashStorage;

/// The updater one project owns.
///
/// Declared as a `static` by the generated `src/generated.rs` and passed to
/// [`Ota::new_in`], so two projects (or a host test) never share one cell. The
/// `critical_section` mutex is what makes it `Sync`: the generated `Context`
/// is borrowed by embassy tasks.
pub struct OtaStorage {
    inner: Mutex<RefCell<Option<Updater<FlashStorage<'static>>>>>,
}

impl OtaStorage {
    /// An empty storage; [`Ota::new_in`] fills it at boot.
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(RefCell::new(None)),
        }
    }
}

impl Default for OtaStorage {
    fn default() -> Self {
        Self::new()
    }
}

/// Why an OTA operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtaError {
    /// The updater was never built: `main()` could not create it at boot.
    NotInitialized,
    /// `esp-hal-ota` refused the operation (CRC, verification, flash, size).
    Updater(esp_hal_ota::Error),
}

impl core::fmt::Display for OtaError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            OtaError::NotInitialized => f.write_str("OTA updater is not initialized"),
            OtaError::Updater(error) => write!(f, "OTA update failed: {error}"),
        }
    }
}

impl From<esp_hal_ota::Error> for OtaError {
    fn from(error: esp_hal_ota::Error) -> Self {
        OtaError::Updater(error)
    }
}

/// A handle to the board's OTA updater. `Copy`, so it can sit on `Context`.
///
/// Created once at boot by the generated `main.rs` from the project's `FLASH`
/// peripheral; `app.rs` then drives `begin` → `write` → `finish` → `reboot`.
#[derive(Clone, Copy)]
pub struct Ota {
    storage: &'static OtaStorage,
    rollback: bool,
}

impl Ota {
    /// Build the updater from the project's `FLASH` peripheral.
    ///
    /// `rollback` is `<ota rollback>`: when true, the booted image must call
    /// [`Ota::mark_app_valid`] or the bootloader reverts to the previous slot
    /// on the next reset.
    ///
    /// Reads the partition table from flash (`0x8000`), so a project without a
    /// 2-slot table is a named error here rather than a corrupt update later.
    pub fn new_in(
        storage: &'static OtaStorage,
        flash: esp_hal::peripherals::FLASH<'static>,
        rollback: bool,
    ) -> Result<Self, OtaError> {
        let updater = Updater::new(FlashStorage::new(flash)).map_err(OtaError::Updater)?;
        critical_section::with(|cs| {
            *storage.inner.borrow_ref_mut(cs) = Some(updater);
        });
        Ok(Self { storage, rollback })
    }

    /// Run `f` against the updater while the storage is borrowed.
    fn with<R>(
        &self,
        f: impl FnOnce(&mut Updater<FlashStorage<'static>>) -> Result<R, esp_hal_ota::Error>,
    ) -> Result<R, OtaError> {
        critical_section::with(|cs| {
            let mut slot = self.storage.inner.borrow_ref_mut(cs);
            let updater = slot.as_mut().ok_or(OtaError::NotInitialized)?;
            f(updater).map_err(OtaError::Updater)
        })
    }

    /// Start an update of an image of `image_size` bytes whose zlib CRC32 is
    /// `target_crc32`. The inactive slot must fit the image.
    pub fn begin(&self, image_size: u32, target_crc32: u32) -> Result<(), OtaError> {
        self.with(|updater| updater.begin(image_size, target_crc32))
    }

    /// Write the next chunk of the image. Returns `true` once the whole
    /// `image_size` has been written.
    pub fn write(&self, chunk: &[u8]) -> Result<bool, OtaError> {
        self.with(|updater| updater.write(chunk))
    }

    /// How much of the image has been written, in percent (`0..=100`).
    pub fn progress(&self) -> Option<u8> {
        self.with(|updater| Ok(updater.progress().map(|progress| progress.percent())))
            .ok()
            .flatten()
    }

    /// Verify the written image (CRC32, then a read-back of flash) and make it
    /// the boot partition. Applies the project's `rollback` setting.
    pub fn finish(&self) -> Result<(), OtaError> {
        self.with(|updater| updater.finish(self.rollback))
    }

    /// Confirm the running image so the bootloader keeps it.
    ///
    /// Only meaningful with rollback enabled: a freshly booted image that
    /// never calls this is reverted on the next reset.
    pub fn mark_app_valid(&self) -> Result<(), OtaError> {
        self.with(|updater| updater.mark_app_valid())
    }

    /// Reset the chip, booting the slot `finish` activated.
    pub fn reboot(&self) -> ! {
        esp_hal::system::software_reset()
    }
}
