#[cfg(windows)]
const ES_CONTINUOUS: u32 = 0x80000000;
#[cfg(windows)]
const ES_SYSTEM_REQUIRED: u32 = 0x00000001;
#[cfg(windows)]
const ES_DISPLAY_REQUIRED: u32 = 0x00000002;

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn SetThreadExecutionState(es_flags: u32) -> u32;
}

pub struct KeepAwakeGuard {
    #[cfg(windows)]
    active: bool,
}

impl KeepAwakeGuard {
    pub fn new() -> Self {
        #[cfg(windows)]
        {
            let flags = ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED;
            let active = unsafe { SetThreadExecutionState(flags) } != 0;
            Self { active }
        }

        #[cfg(not(windows))]
        {
            Self {}
        }
    }
}

impl Drop for KeepAwakeGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            if self.active {
                unsafe {
                    SetThreadExecutionState(ES_CONTINUOUS);
                }
            }
        }
    }
}
