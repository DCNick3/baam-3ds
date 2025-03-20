#[derive(Debug, Default)]
#[repr(C)]
pub struct ProcessingTimings {
    pub binarize: u64,
    pub linear_external_contours: u64,
    pub contour_processing: u64,
    pub qr_detection: u64,
    pub qr_decoding: u64,
}

unsafe extern "C" {
    fn alloc_qr_processing_context(width: u32, height: u32) -> *mut std::ffi::c_void;
    fn free_qr_processing_context(raw_context: *mut std::ffi::c_void);
    fn qr_processing_set_frame(raw_context: *mut std::ffi::c_void, yuv_ptr: *const u8);
    fn qr_processing_process(raw_context: *mut std::ffi::c_void, timings: &mut ProcessingTimings);
    fn qr_processing_get_strings(
        raw_context: *mut std::ffi::c_void,
        out_ptr: &mut *const *const std::ffi::c_char,
        out_count: &mut u32,
    );
}

pub struct Context(*mut std::ffi::c_void);

impl Context {
    pub fn new(width: u32, height: u32) -> Self {
        let raw_context = unsafe { alloc_qr_processing_context(width, height) };
        Self(raw_context)
    }

    pub unsafe fn set_frame(&mut self, frame_ptr: *const u8) {
        unsafe { qr_processing_set_frame(self.0, frame_ptr) }
    }

    pub fn process(&mut self, timings: &mut ProcessingTimings) {
        unsafe { qr_processing_process(self.0, timings) }
    }

    pub fn get_strings(&mut self) -> Vec<String> {
        let mut ptr: *const *const std::ffi::c_char = std::ptr::null();
        let mut count = 0;
        unsafe {
            qr_processing_get_strings(self.0, &mut ptr, &mut count);
        }

        let strings_slice = unsafe { std::slice::from_raw_parts(ptr, count as usize) };

        strings_slice
            .iter()
            .map(|&ptr| unsafe { std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned() })
            .collect()
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe { free_qr_processing_context(self.0) }
    }
}

unsafe impl Send for Context {}
unsafe impl Sync for Context {}
