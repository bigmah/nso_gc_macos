//! Nintendo "SW2" protocol shared by the Switch 2 controller family.
//!
//! The NSO GameCube controller (VID 0x057E, PID 0x2073) does not speak plain
//! HID for its useful data. Both transports carry the same command frames and
//! the same input report, so everything that is not framing lives here.
//!
//! Sources: SDL's `SDL_hidapi_switch2.c`, ndeadly's switch2 research, BlueRetro,
//! and the nsogcd protocol notes.

pub mod calibration;
pub mod command;
pub mod report;
pub mod state;

/// USB vendor ID for all Nintendo controllers.
pub const VID_NINTENDO: u16 = 0x057E;
/// USB product ID for the NSO GameCube controller.
pub const PID_NSO_GAMECUBE: u16 = 0x2073;
