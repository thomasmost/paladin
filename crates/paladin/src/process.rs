//! Process-level housekeeping for the browser process.

/// Make every process this one starts die with it.
///
/// Servo's content processes are children of the browser process (see
/// `paladin_engine::Engine::new`). A clean exit stops them through the
/// engine, but a browser that is killed or crashes natively would leave
/// them running. On Windows this puts the browser into a job object with
/// `KILL_ON_JOB_CLOSE`; children join the job automatically and the kernel
/// ends them when the browser's handle to the job goes away, which is when
/// the browser process ends for any reason. The job handle is never closed
/// on purpose. On other platforms the content process watches its parent
/// instead (see `paladin_engine::run_content_process`).
///
/// Call it before the engine opens the first tab. Failure (for example a
/// parent job that forbids nesting) is logged and the browser runs on
/// without the guarantee.
#[cfg(windows)]
pub fn bind_children_to_this_process() {
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};

    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // SAFETY: plain Win32 calls with valid arguments; `info` is a fully
    // initialised, correctly sized struct, and the job handle is leaked
    // deliberately so the job outlives this function.
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            log::warn!(
                "cannot create the job object for content processes (error {})",
                GetLastError()
            );
            return;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw const info).cast::<c_void>(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
        {
            log::warn!(
                "cannot configure the job object for content processes (error {})",
                GetLastError()
            );
            return;
        }
        if AssignProcessToJobObject(job, GetCurrentProcess()) == 0 {
            log::warn!(
                "cannot join the job object for content processes (error {}); \
                 content processes will outlive a killed browser",
                GetLastError()
            );
        }
    }
}

#[cfg(not(windows))]
pub fn bind_children_to_this_process() {}
