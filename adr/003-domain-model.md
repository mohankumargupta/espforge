# ADR-003 — Core domain model & ubiquitous language

**Status:** accepted

**Decision.** espforge's core model is a **3-tier typed spine**: Peripheral (raw
hardware) → Component (reusable named capability with an API — hardware-backed *or*
software-service) → Device (terminal high-level driver consuming components ± pins).
Wiring forms a DAG: components may be consumed by components and devices; devices
are terminal (consumed only by the app). Every instance is typed
(component_kind / device_kind / resource kind), not `driver: String` + `Value` — so
validation and dependency ordering become structural. A single inspectable **IR
(DeviceTree)** is the artifact all emitters read.

**Ubiquitous language.**
- Peripheral — raw ESP32 hardware resource (pin, I2C/SPI/UART bus, WiFi)
- Component — reusable named capability with an API; hardware-backed or
  software-service (http, mqtt, websockets, voice_control, accelerometer)
- Device — terminal high-level driver consuming components ± pins
- Instance — one named occurrence of a component/device in a project
- ResourceRef / PinRef — typed reference value object to a named resource
- Project — the whole spec: metadata + peripherals + components + devices + app
- IR / DeviceTree — validated intermediate representation all emitters read

**Boundary rule asserted.** Devices are terminal — a device may not be consumed by
another device, only by the app. (Deliberate simplification vs esphome, keeps the
DAG acyclic.)

**Drivers.** The tier model mirrors how embedded firmware is built and how
datasheets describe hardware. B's two-tier collapse loses a real distinction; C's
pure graph is a generality tax.
