use std::{
    ffi::{CStr, CString, c_void},
    mem::MaybeUninit,
    ptr::NonNull,
    slice::from_raw_parts,
    thread,
    time::Duration,
};

use ndk_sys::{
    ACameraCaptureSession, ACameraCaptureSession_captureCallbacks, ACameraCaptureSession_close,
    ACameraCaptureSession_setRepeatingRequest, ACameraCaptureSession_stateCallbacks, ACameraDevice,
    ACameraDevice_close, ACameraDevice_createCaptureRequest, ACameraDevice_createCaptureSession,
    ACameraDevice_request_template, ACameraDevice_stateCallbacks, ACameraIdList, ACameraManager,
    ACameraManager_create, ACameraManager_delete, ACameraManager_deleteCameraIdList,
    ACameraManager_getCameraCharacteristics, ACameraManager_getCameraIdList,
    ACameraManager_openCamera, ACameraMetadata, ACameraMetadata_const_entry, ACameraMetadata_free,
    ACameraMetadata_getConstEntry, ACameraOutputTarget, ACameraOutputTarget_create,
    ACameraOutputTarget_free, ACaptureRequest, ACaptureRequest_addTarget, ACaptureRequest_free,
    ACaptureRequest_removeTarget, ACaptureSessionOutput, ACaptureSessionOutput_create,
    ACaptureSessionOutput_free, ACaptureSessionOutputContainer, ACaptureSessionOutputContainer_add,
    ACaptureSessionOutputContainer_create, ACaptureSessionOutputContainer_free,
    ACaptureSessionOutputContainer_remove, AIMAGE_FORMATS, AImage, AImage_delete,
    AImage_getPlaneData, AImageReader, AImageReader_ImageListener, AImageReader_acquireLatestImage,
    AImageReader_delete, AImageReader_getWindow, AImageReader_new, AImageReader_setImageListener,
    ANativeWindow, acamera_metadata_enum_acamera_lens_facing, acamera_metadata_tag,
};
use slint::{Rgba8Pixel, SharedPixelBuffer, Weak};

use crate::MainWindow;

pub enum ImageRotation {
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

impl ImageRotation {
    pub fn from_deg(rotation: i32) -> Self {
        match rotation {
            45..135 => Self::Deg90,
            135..225 => Self::Deg180,
            225..315 => Self::Deg270,
            // 0..45 && 315..360
            _ => Self::Deg0,
        }
    }
}

// TODO: Handle drop in more explicit way. Currently the fields needs a specific
//       order to ensure they are dropped in correct order (e.g. children before parents).
pub struct CameraContext {
    pub session: CaptureSession,
    pub device: CameraDevice,
    pub manager: CameraManager,
    pub image_reader: ImageReader,
}

pub struct ImageHandler {
    main_window: Weak<MainWindow>,
    stream_configuration: StreamConfiguration,
    // TODO: Handle dynamically. For example if user rotates phone after the camera is started.
    rotation: ImageRotation,
}

impl ImageHandler {
    pub fn new(
        main_window: Weak<MainWindow>,
        stream_configuration: StreamConfiguration,
        rotation: ImageRotation,
    ) -> Self {
        Self {
            main_window,
            stream_configuration,
            rotation,
        }
    }

    pub fn on_image_available(&mut self, reader: *mut AImageReader) {
        let pixel_buffer = {
            let image = self.acquire_latest_image(reader);
            self.create_slint_image(&image, &self.rotation)
        };

        // https://github.com/slint-ui/slint/issues/1649
        self.main_window
            .upgrade_in_event_loop(move |window| {
                window.set_camera(slint::Image::from_rgba8(pixel_buffer));
            })
            .unwrap();
    }

    fn acquire_latest_image(&self, reader: *mut AImageReader) -> Image {
        let mut image = MaybeUninit::uninit();
        let camera_status = unsafe { AImageReader_acquireLatestImage(reader, image.as_mut_ptr()) };

        if camera_status.0 != 0 {
            panic!("NOT GOOD: {}", camera_status.0);
        }

        unsafe { Image::new(NonNull::new(image.assume_init()).unwrap()) }
    }

    fn create_slint_image(
        &self,
        image: &Image,
        rotation: &ImageRotation,
    ) -> SharedPixelBuffer<Rgba8Pixel> {
        let mut data = MaybeUninit::uninit();
        let mut data_len = MaybeUninit::uninit();
        let camera_status = unsafe {
            AImage_getPlaneData(
                image.image.as_ptr(),
                0,
                data.as_mut_ptr(),
                data_len.as_mut_ptr(),
            )
        };

        if camera_status.0 != 0 {
            panic!("NOT GOOD: {}", camera_status.0);
        }

        let (dynamic_image, width, height) = {
            let buffer =
                unsafe { from_raw_parts(data.assume_init(), data_len.assume_init() as usize) };
            let image = image::load_from_memory(buffer).unwrap();

            let sc = &self.stream_configuration;
            match rotation {
                ImageRotation::Deg0 => (image, sc.width, sc.height),
                ImageRotation::Deg90 => (image.rotate90(), sc.height, sc.width),
                ImageRotation::Deg180 => (image.rotate180(), sc.width, sc.height),
                ImageRotation::Deg270 => (image.rotate270(), sc.height, sc.width),
            }
        };

        SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
            dynamic_image.into_rgba8().as_raw(),
            width as u32,
            height as u32,
        )
    }
}

pub struct CameraIdList {
    list: NonNull<ACameraIdList>,
}

impl CameraIdList {
    pub fn new(list: NonNull<ACameraIdList>) -> Self {
        Self { list }
    }
}

impl Drop for CameraIdList {
    fn drop(&mut self) {
        unsafe {
            ACameraManager_deleteCameraIdList(self.list.as_ptr());
        }
    }
}

pub struct CameraDevice {
    device: NonNull<ACameraDevice>,
    container: Option<OutputContainer>,
}

unsafe impl Send for CameraDevice {}
unsafe impl Sync for CameraDevice {}

impl CameraDevice {
    pub fn new(device: NonNull<ACameraDevice>) -> Self {
        Self {
            device,
            container: None,
        }
    }

    pub fn create_request(&self) -> CaptureRequest {
        let mut capture_request = MaybeUninit::uninit();
        let camera_status = unsafe {
            ACameraDevice_createCaptureRequest(
                self.device.as_ptr(),
                ACameraDevice_request_template::TEMPLATE_PREVIEW,
                capture_request.as_mut_ptr(),
            )
        };

        if camera_status.0 != 0 {
            panic!("NOT GOOD: {}", camera_status.0);
        }

        CaptureRequest::new(unsafe { NonNull::new(capture_request.assume_init()).unwrap() })
    }

    pub fn create_session(&mut self, container: OutputContainer) -> CaptureSession {
        unsafe extern "C" fn no_op(_: *mut c_void, _: *mut ACameraCaptureSession) {
            eprintln!("ACameraDevice_createCaptureSession-no_op");
        }

        let callbacks = ACameraCaptureSession_stateCallbacks {
            // TODO: Handle
            context: std::ptr::null_mut(),
            onClosed: Some(no_op),
            onReady: Some(no_op),
            onActive: Some(no_op),
        };

        let mut session = MaybeUninit::uninit();
        let camera_status = unsafe {
            ACameraDevice_createCaptureSession(
                self.device.as_ptr(),
                container.container.as_ptr(),
                &callbacks,
                session.as_mut_ptr(),
            )
        };

        if camera_status.0 != 0 {
            panic!("NOT GOOD: {}", camera_status.0);
        }

        self.container = Some(container);

        unsafe { CaptureSession::new(NonNull::new(session.assume_init()).unwrap()) }
    }
}

impl Drop for CameraDevice {
    fn drop(&mut self) {
        unsafe {
            std::mem::drop(std::mem::take(&mut self.container));
            ACameraDevice_close(self.device.as_ptr());
        }
    }
}

pub struct CaptureRequest {
    request: NonNull<ACaptureRequest>,
    target: Option<OutputTarget>,
}

unsafe impl Send for CaptureRequest {}
unsafe impl Sync for CaptureRequest {}

impl CaptureRequest {
    pub fn new(request: NonNull<ACaptureRequest>) -> Self {
        Self {
            request,
            target: None,
        }
    }

    pub fn add_target(&mut self, target: OutputTarget) {
        let camera_status =
            unsafe { ACaptureRequest_addTarget(self.request.as_ptr(), target.target.as_ptr()) };

        if camera_status.0 != 0 {
            panic!("NOT GOOD: {}", camera_status.0);
        }

        self.target = Some(target);
    }
}

impl Drop for CaptureRequest {
    fn drop(&mut self) {
        unsafe {
            if let Some(target) = &self.target {
                ACaptureRequest_removeTarget(self.request.as_ptr(), target.target.as_ptr());
            }
            ACaptureRequest_free(self.request.as_ptr());
        }
    }
}

pub struct CameraMetadata {
    metadata: NonNull<ACameraMetadata>,
}

impl CameraMetadata {
    pub fn new(metadata: NonNull<ACameraMetadata>) -> Self {
        Self { metadata }
    }
}

impl Drop for CameraMetadata {
    fn drop(&mut self) {
        unsafe {
            ACameraMetadata_free(self.metadata.as_ptr());
        }
    }
}

pub struct Image {
    image: NonNull<AImage>,
}

unsafe impl Send for Image {}
unsafe impl Sync for Image {}

impl Image {
    pub fn new(image: NonNull<AImage>) -> Self {
        Self { image }
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        unsafe {
            AImage_delete(self.image.as_ptr());
        }
    }
}

pub struct StreamConfiguration {
    format: i32,
    width: i32,
    height: i32,
}

impl StreamConfiguration {
    pub fn new(format: i32, width: i32, height: i32) -> Self {
        Self {
            format,
            width,
            height,
        }
    }
}

pub struct ImageReader {
    reader: NonNull<AImageReader>,
    listener: Option<AImageReader_ImageListener>,
}

unsafe impl Send for ImageReader {}
unsafe impl Sync for ImageReader {}

impl ImageReader {
    pub fn new(reader: NonNull<AImageReader>) -> Self {
        Self {
            reader,
            listener: None,
        }
    }

    pub fn get_window(&self) -> NativeWindow {
        let mut window = MaybeUninit::uninit();
        let camera_status =
            unsafe { AImageReader_getWindow(self.reader.as_ptr(), window.as_mut_ptr()) };

        if camera_status.0 != 0 {
            panic!("NOT GOOD: {}", camera_status.0);
        }

        unsafe { NativeWindow::new(NonNull::new(window.assume_init()).unwrap()) }
    }

    pub fn add_listener(&mut self, image_handler: *mut ImageHandler) {
        unsafe extern "C" fn on_image_available(context: *mut c_void, reader: *mut AImageReader) {
            let image_handler = context as *mut ImageHandler;
            unsafe {
                (*image_handler).on_image_available(reader);
            }
        }

        let mut listener = AImageReader_ImageListener {
            // TODO: Handle
            context: image_handler as *mut _,
            onImageAvailable: Some(on_image_available),
        };

        let camera_status =
            unsafe { AImageReader_setImageListener(self.reader.as_ptr(), &mut listener) };

        if camera_status.0 != 0 {
            panic!("NOT GOOD: {}", camera_status.0);
        }

        self.listener = Some(listener);
    }
}

impl Drop for ImageReader {
    fn drop(&mut self) {
        unsafe {
            // TODO: Implement way to ensure that this AImageReader isn't closed before
            //       all active AImage's are closed (used in side `self.listener`).
            thread::sleep(Duration::from_millis(100));
            AImageReader_delete(self.reader.as_ptr());
        }
    }
}

pub struct NativeWindow {
    window: NonNull<ANativeWindow>,
}

unsafe impl Send for NativeWindow {}
unsafe impl Sync for NativeWindow {}

impl NativeWindow {
    pub fn new(window: NonNull<ANativeWindow>) -> Self {
        Self { window }
    }

    pub fn create_container(&self) -> OutputContainer {
        let mut output_container = MaybeUninit::uninit();
        let camera_status =
            unsafe { ACaptureSessionOutputContainer_create(output_container.as_mut_ptr()) };

        if camera_status.0 != 0 {
            panic!("NOT GOOD: {}", camera_status.0);
        }

        let mut container =
            unsafe { OutputContainer::new(NonNull::new(output_container.assume_init()).unwrap()) };

        let mut output = MaybeUninit::uninit();
        let camera_status =
            unsafe { ACaptureSessionOutput_create(self.window.as_ptr(), output.as_mut_ptr()) };

        if camera_status.0 != 0 {
            panic!("NOT GOOD: {}", camera_status.0);
        }

        let output = unsafe { SessionOutput::new(NonNull::new(output.assume_init()).unwrap()) };

        let camera_status = unsafe {
            ACaptureSessionOutputContainer_add(container.container.as_ptr(), output.output.as_ptr())
        };

        if camera_status.0 != 0 {
            panic!("NOT GOOD: {}", camera_status.0);
        }

        container.output = Some(output);
        container
    }

    pub fn create_target(&self) -> OutputTarget {
        let mut target = MaybeUninit::uninit();
        let camera_status =
            unsafe { ACameraOutputTarget_create(self.window.as_ptr(), target.as_mut_ptr()) };

        if camera_status.0 != 0 {
            panic!("NOT GOOD: {}", camera_status.0);
        }

        unsafe { OutputTarget::new(NonNull::new(target.assume_init()).unwrap()) }
    }
}

pub struct OutputContainer {
    container: NonNull<ACaptureSessionOutputContainer>,
    output: Option<SessionOutput>,
}

unsafe impl Send for OutputContainer {}
unsafe impl Sync for OutputContainer {}

impl OutputContainer {
    pub fn new(container: NonNull<ACaptureSessionOutputContainer>) -> Self {
        Self {
            container,
            output: None,
        }
    }
}

impl Drop for OutputContainer {
    fn drop(&mut self) {
        unsafe {
            if let Some(output) = &self.output {
                ACaptureSessionOutputContainer_remove(
                    self.container.as_ptr(),
                    output.output.as_ptr(),
                );
            }
            std::mem::drop(std::mem::take(&mut self.output));
            ACaptureSessionOutputContainer_free(self.container.as_ptr());
        }
    }
}

pub struct OutputTarget {
    target: NonNull<ACameraOutputTarget>,
}

unsafe impl Send for OutputTarget {}
unsafe impl Sync for OutputTarget {}

impl OutputTarget {
    pub fn new(target: NonNull<ACameraOutputTarget>) -> Self {
        Self { target }
    }
}

impl Drop for OutputTarget {
    fn drop(&mut self) {
        unsafe {
            ACameraOutputTarget_free(self.target.as_ptr());
        }
    }
}

pub struct SessionOutput {
    output: NonNull<ACaptureSessionOutput>,
}

unsafe impl Send for SessionOutput {}
unsafe impl Sync for SessionOutput {}

impl SessionOutput {
    pub fn new(output: NonNull<ACaptureSessionOutput>) -> Self {
        Self { output }
    }
}

impl Drop for SessionOutput {
    fn drop(&mut self) {
        unsafe {
            ACaptureSessionOutput_free(self.output.as_ptr());
        }
    }
}

pub struct CaptureSession {
    session: NonNull<ACameraCaptureSession>,
    request: Option<CaptureRequest>,
}

unsafe impl Send for CaptureSession {}
unsafe impl Sync for CaptureSession {}

impl CaptureSession {
    pub fn new(session: NonNull<ACameraCaptureSession>) -> Self {
        Self {
            session,
            request: None,
        }
    }

    pub fn start(&mut self, request: CaptureRequest) {
        let mut callbacks = ACameraCaptureSession_captureCallbacks {
            // TODO: Handle
            context: std::ptr::null_mut(),
            onCaptureStarted: None,
            onCaptureProgressed: None,
            onCaptureCompleted: None,
            onCaptureFailed: None,
            onCaptureSequenceCompleted: None,
            onCaptureSequenceAborted: None,
            onCaptureBufferLost: None,
        };

        let mut capture_sequence_id = MaybeUninit::uninit();
        let mut requests = [request.request.as_ptr()];

        let camera_status = unsafe {
            ACameraCaptureSession_setRepeatingRequest(
                self.session.as_ptr(),
                &mut callbacks,
                requests.len() as i32,
                requests.as_mut_ptr(),
                capture_sequence_id.as_mut_ptr(),
            )
        };

        if camera_status.0 != 0 {
            panic!("NOT GOOD: {}", camera_status.0);
        }

        self.request = Some(request);
    }
}

impl Drop for CaptureSession {
    fn drop(&mut self) {
        unsafe {
            ACameraCaptureSession_close(self.session.as_ptr());
        }
    }
}

// https://github.com/rust-mobile/ndk/pull/77
// https://github.com/android/ndk-samples/tree/c1139f68c459adfc7cf5deb310fe9266a248d200/camera/basic/src/main/cpp
pub struct CameraManager {
    manager: NonNull<ACameraManager>,
}

unsafe impl Send for CameraManager {}
unsafe impl Sync for CameraManager {}

impl CameraManager {
    pub fn new() -> Self {
        Self {
            manager: unsafe { NonNull::new(ACameraManager_create()).unwrap() },
        }
    }

    // Returns the first back-facing camera.
    pub fn get_camera_id(&self) -> CString {
        let camera_ids = self.camera_id_list();
        let list = unsafe { *camera_ids.list.as_ptr() };

        let mut selected_camera_id = None;

        eprintln!("FOUND NUMCAMERAS: {}", list.numCameras as isize);

        for i in 0..list.numCameras as isize {
            let camera_id = unsafe { CStr::from_ptr(*list.cameraIds.offset(i)) };

            let metadata = self.get_metadata(camera_id);
            let entry =
                self.get_metadata_entry(&metadata, acamera_metadata_tag::ACAMERA_LENS_FACING);

            let facing = unsafe { *entry.data.u8_.offset(0) } as u32;
            if facing == acamera_metadata_enum_acamera_lens_facing::ACAMERA_LENS_FACING_BACK.0 {
                selected_camera_id = Some(camera_id.into());
            }

            eprintln!("CAMERA ID: {:?}, facing: {}", camera_id, facing);
        }

        selected_camera_id.unwrap()
    }

    pub fn camera_id_list(&self) -> CameraIdList {
        let mut camera_id_list = MaybeUninit::uninit();
        let camera_status = unsafe {
            ACameraManager_getCameraIdList(self.manager.as_ptr(), camera_id_list.as_mut_ptr())
        };

        if camera_status.0 != 0 {
            panic!("NOT GOOD: {}", camera_status.0);
        }

        CameraIdList::new(unsafe { NonNull::new(camera_id_list.assume_init()).unwrap() })
    }

    pub fn open_camera(&self, camera_id: &CStr) -> CameraDevice {
        unsafe extern "C" fn no_op(_: *mut c_void, _: *mut ACameraDevice) {
            eprintln!("ACameraManager_openCamera-no_op");
        }

        unsafe extern "C" fn no_op2(_: *mut c_void, _: *mut ACameraDevice, _: i32) {
            eprintln!("ACameraManager_openCamera-no_op2");
        }

        let mut callbacks = ACameraDevice_stateCallbacks {
            // TODO: Handle
            context: std::ptr::null_mut(),
            onDisconnected: Some(no_op),
            onError: Some(no_op2),
        };

        let mut device = MaybeUninit::uninit();
        let camera_status = unsafe {
            ACameraManager_openCamera(
                self.manager.as_ptr(),
                camera_id.as_ptr(),
                &mut callbacks,
                device.as_mut_ptr(),
            )
        };

        if camera_status.0 != 0 {
            panic!("NOT GOOD: {}", camera_status.0);
        }

        unsafe { CameraDevice::new(NonNull::new(device.assume_init()).unwrap()) }
    }

    pub fn create_image_reader(&self, stream_configuration: &StreamConfiguration) -> ImageReader {
        let mut image_reader = MaybeUninit::uninit();
        let camera_status = unsafe {
            AImageReader_new(
                stream_configuration.width,
                stream_configuration.height,
                stream_configuration.format,
                2,
                image_reader.as_mut_ptr(),
            )
        };

        if camera_status.0 != 0 {
            panic!("NOT GOOD: {}", camera_status.0);
        }

        unsafe { ImageReader::new(NonNull::new(image_reader.assume_init()).unwrap()) }
    }

    pub fn get_stream_configuration(&self, camera_id: &CStr) -> StreamConfiguration {
        let metadata = self.get_metadata(camera_id);
        let entry = self.get_metadata_entry(
            &metadata,
            acamera_metadata_tag::ACAMERA_SCALER_AVAILABLE_STREAM_CONFIGURATIONS,
        );

        let mut stream_config: Option<StreamConfiguration> = None;

        // https://developer.android.com/ndk/reference/group/camera#group___camera_1gga49cf3e5a3deefe079ad036a8fac14627ab4ef4fabbbaaecf6f2fc74eaa9197b26
        for idx in (0..entry.count as isize).step_by(4) {
            let format = unsafe { *entry.data.i32_.offset(idx + 0) };
            let width = unsafe { *entry.data.i32_.offset(idx + 1) };
            let height = unsafe { *entry.data.i32_.offset(idx + 2) };

            // Use "arbitrary" format and smallest resolution.
            if format == AIMAGE_FORMATS::AIMAGE_FORMAT_JPEG.0 as i32 {
                if let Some(s) = &stream_config
                    && width < s.width
                {
                    stream_config = Some(StreamConfiguration::new(format, width, height));
                } else if stream_config.is_none() {
                    stream_config = Some(StreamConfiguration::new(format, width, height));
                }
            }
        }

        stream_config.unwrap()
    }

    pub fn camera_rotation(&self, camera_id: &CStr) -> i32 {
        let metadata = self.get_metadata(camera_id);
        let entry =
            self.get_metadata_entry(&metadata, acamera_metadata_tag::ACAMERA_SENSOR_ORIENTATION);

        unsafe { *entry.data.i32_.offset(0) }
    }

    fn get_metadata(&self, camera_id: &CStr) -> CameraMetadata {
        let mut metadata = MaybeUninit::uninit();
        let camera_status = unsafe {
            ACameraManager_getCameraCharacteristics(
                self.manager.as_ptr(),
                camera_id.as_ptr(),
                metadata.as_mut_ptr(),
            )
        };

        if camera_status.0 != 0 {
            panic!("NOT GOOD: {}", camera_status.0);
        }

        unsafe { CameraMetadata::new(NonNull::new(metadata.assume_init()).unwrap()) }
    }

    fn get_metadata_entry(
        &self,
        metadata: &CameraMetadata,
        tag: acamera_metadata_tag,
    ) -> ACameraMetadata_const_entry {
        let mut const_entry = MaybeUninit::uninit();
        let camera_status = unsafe {
            ACameraMetadata_getConstEntry(
                metadata.metadata.as_ptr(),
                tag.0,
                const_entry.as_mut_ptr(),
            )
        };

        if camera_status.0 != 0 {
            panic!("NOT GOOD: {}", camera_status.0);
        }

        unsafe { const_entry.assume_init() }
    }
}

impl Drop for CameraManager {
    fn drop(&mut self) {
        unsafe {
            ACameraManager_delete(self.manager.as_ptr());
        }
    }
}
