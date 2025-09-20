import java.nio.ByteBuffer;
import java.util.Arrays;
import java.util.concurrent.Executor;
import java.util.concurrent.Executors;

import android.app.Activity;
import android.hardware.camera2.CameraAccessException;
import android.hardware.camera2.CameraCaptureSession;
import android.hardware.camera2.CameraCharacteristics;
import android.hardware.camera2.CameraDevice;
import android.hardware.camera2.CameraManager;
import android.hardware.camera2.CameraMetadata;
import android.hardware.camera2.CaptureRequest;
import android.hardware.camera2.params.OutputConfiguration;
import android.hardware.camera2.params.SessionConfiguration;
import android.hardware.camera2.params.StreamConfigurationMap;
import android.media.Image;
import android.media.ImageReader;
import android.media.Image.Plane;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;
import android.util.Size;
import android.view.Surface;
import android.view.WindowManager;

// https://developer.android.com/reference/android/hardware/camera2/package-summary
// https://github.com/android/camera-samples/blob/main/Camera2Basic/app/src/main/java/com/example/android/camera2/basic/fragments/CameraFragment.kt
// https://github.com/slint-ui/slint/blob/master/internal/backends/android-activity/java/SlintAndroidJavaHelper.java
// https://github.com/jni-rs/jni-rs/tree/master/example

class StreamConfiguration {
    private final int format, width, height;

    public StreamConfiguration(int format, int width, int height) {
        this.format = format;
        this.width = width;
        this.height = height;
    }

    public int getFormat() {
        return this.format;
    }

    public int getWidth() {
        return this.width;
    }

    public int getHeight() {
        return this.height;
    }
}

class CameraDeviceHandler extends CameraDevice.StateCallback {
    private final CameraHelper cameraHelper;

    public CameraDeviceHandler(CameraHelper cameraHelper) {
        this.cameraHelper = cameraHelper;
    }

    @Override
    public void onClosed(CameraDevice device) {
        Log.d(this.getClass().getName(), "onClosed");
    }

    @Override
    public void onDisconnected(CameraDevice device) {
        Log.d(this.getClass().getName(), "onDisconnected");
    }

    @Override
    public void onError(CameraDevice device, int error) {
        Log.d(this.getClass().getName(), "onError, code: " + error);
    }

    @Override
    public void onOpened(CameraDevice device) {
        Log.d(this.getClass().getName(), "onOpened");
        cameraHelper.setupDevice(device);
    }
}

class CameraSessionHandler extends CameraCaptureSession.StateCallback {
    private final CameraHelper cameraHelper;
    private final Surface surface;

    public CameraSessionHandler(CameraHelper cameraHelper, Surface surface) {
        this.cameraHelper = cameraHelper;
        this.surface = surface;
    }

    @Override
    public void onConfigureFailed(CameraCaptureSession session) {
        Log.d(this.getClass().getName(), "onConfigureFailed");
    }

    @Override
    public void onConfigured(CameraCaptureSession session) {
        Log.d(this.getClass().getName(), "onConfigured");
        this.cameraHelper.setupSession(session, this.surface);
    }
}

public class CameraHelper {
    public final static String CAMERA_SERVICE = "camera";
    public final static String WINDOW_SERVICE = "window";
    public final static int IMAGE_FORMAT_JPEG = 256;
    public final static int IMAGE_FORMAT_YUV_420_888 = 35;
    public final static int TEMPLATE_PREVIEW = 1;

    private final Activity activity;

    private final Executor executor;

    private boolean isRunning;
    private String cameraId;
    private CameraDevice device;
    private ImageReader imageReader;

    /*
     * Pointer to rust object of type `std::rc::Rc<AppState>`.
     */
    private long appState;

    public CameraHelper(Activity activity) {
        this.activity = activity;
        this.executor = Executors.newCachedThreadPool();
    }

    public static native void handleImage(long slintMainWindow, byte[] planeY, int strideY, byte[] planeU, int strideU,
            byte[] planeV, int strideV, int rotation, int width, int height);

    public boolean isRunning() {
        return this.isRunning;
    }

    public void start(long appState) throws CameraAccessException {
        CameraManager cameraManager = (CameraManager) this.activity.getSystemService(CAMERA_SERVICE);

        this.appState = appState;
        this.cameraId = this.selectCameraId(cameraManager);

        cameraManager.openCamera(cameraId, this.executor, new CameraDeviceHandler(this));
    }

    public void stop() {
        this.isRunning = false;

        if (this.imageReader != null) {
            this.imageReader.close();
            this.imageReader = null;
        }

        if (this.device != null) {
            this.device.close();
            this.device = null;
        }
    }

    void setupDevice(CameraDevice device) {
        this.device = device;

        try {
            CameraManager cameraManager = (CameraManager) this.activity.getSystemService(CAMERA_SERVICE);
            CameraCharacteristics characteristics = cameraManager.getCameraCharacteristics(cameraId);

            // Calculation assumes back facing camera.
            int phoneRotation = this.phoneRotation();
            int cameraRotation = characteristics.get(CameraCharacteristics.SENSOR_ORIENTATION);
            int imageRotation = (cameraRotation + phoneRotation + 360) % 360;

            StreamConfiguration streamConfiguration = this.selectStreamConfiguration(characteristics);
            int width = streamConfiguration.getWidth();
            int height = streamConfiguration.getHeight();
            int format = streamConfiguration.getFormat();

            this.imageReader = ImageReader.newInstance(width, height, format, 1);
            this.imageReader.setOnImageAvailableListener((reader) -> {
                byte[][] data = new byte[3][];
                int[] stride = new int[3];

                try (Image image = this.imageReader.acquireNextImage()) {
                    Plane[] planes = image.getPlanes();
                    for (int i = 0; i < 3; i++) {
                        ByteBuffer buffer = planes[i].getBuffer();
                        data[i] = new byte[buffer.remaining()];
                        buffer.get(data[i]);
                        stride[i] = planes[i].getRowStride();
                    }
                }

                handleImage(this.appState, data[0], stride[0], data[1], stride[1], data[2], stride[2],
                        imageRotation, width, height);
            }, new Handler(Looper.getMainLooper()));

            Surface surface = this.imageReader.getSurface();
            SessionConfiguration sessionConfiguration = new SessionConfiguration(SessionConfiguration.SESSION_REGULAR,
                    Arrays.asList(new OutputConfiguration(surface)), this.executor,
                    new CameraSessionHandler(this, surface));

            this.device.createCaptureSession(sessionConfiguration);
        } catch (Exception e) {
            throw new RuntimeException(e); // TODO
        }
    }

    void setupSession(CameraCaptureSession session, Surface surface) {
        try {
            CaptureRequest.Builder requestBuilder = device.createCaptureRequest(TEMPLATE_PREVIEW);
            requestBuilder.addTarget(surface);
            session.setRepeatingRequest(requestBuilder.build(), null, null);
            
            this.isRunning = true;
        } catch (Exception e) {
            throw new RuntimeException(e); // TODO
        }
    }

    private String selectCameraId(CameraManager cameraManager) throws CameraAccessException {
        String[] cameraIds = cameraManager.getCameraIdList();

        for (String cameraId : cameraIds) {
            CameraCharacteristics characteristics = cameraManager.getCameraCharacteristics(cameraId);
            Integer lensFacing = characteristics.get(CameraCharacteristics.LENS_FACING);
            if (lensFacing == CameraMetadata.LENS_FACING_BACK) {
                return cameraId;
            }
        }

        throw new CameraAccessException(CameraAccessException.CAMERA_ERROR, "TODO: Unable to find back facing camera");
    }

    private StreamConfiguration selectStreamConfiguration(CameraCharacteristics characteristics) {
        StreamConfigurationMap streamConfiguration = characteristics
                .get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP);

        Size selectedSize = null;

        // Selects the smallest size.
        Size[] sizes = streamConfiguration.getOutputSizes(IMAGE_FORMAT_YUV_420_888);
        for (Size size : sizes) {
            int width = size.getWidth();
            if (selectedSize == null || selectedSize.getWidth() > width) {
                selectedSize = size;
            }
        }

        return new StreamConfiguration(IMAGE_FORMAT_YUV_420_888, selectedSize.getWidth(), selectedSize.getHeight());
    }

    private int phoneRotation() {
        WindowManager windowManager = (WindowManager) this.activity.getSystemService(WINDOW_SERVICE);
        return windowManager.getDefaultDisplay().getRotation();
    }
}
