import java.io.File;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.List;

import android.app.Activity;

/**
 * Reads/writes OTP auth URLs from/to disk.
 */
public class OtpAuthHelper {
    public static final String FILENAME = "otpauth";

    private final Activity activity;

    public OtpAuthHelper(Activity activity) {
        this.activity = activity;
    }

    public void add(String url) throws IOException {
        List<String> urls = read();
        urls.add(url);
        write(urls);
    }

    public void remove(String url) throws IOException {
        List<String> urls = read();
        urls.remove(url);
        write(urls);
    }

    public void edit(String oldUrl, String newUrl) throws IOException {
        List<String> urls = read();
        int index = urls.indexOf(oldUrl);
        urls.remove(index);
        urls.add(newUrl);
        write(urls);
    }

    public void swap(String firstUrl, String secondUrl) throws IOException {
        List<String> urls = read();
        int firstIndex = urls.indexOf(firstUrl);
        int secondIndex = urls.indexOf(secondUrl);
        urls.set(firstIndex, secondUrl);
        urls.set(secondIndex, firstUrl);
        write(urls);
    }

    public String[] get() throws IOException {
        return read().toArray(new String[0]);
    }

    private List<String> read() throws IOException {
        File file = new File(this.activity.getFilesDir(), FILENAME);
        if (file.exists()) {
            return Files.readAllLines(file.toPath());
        } else {
            return new ArrayList<String>();
        }
    }

    private void write(List<String> urls) throws IOException {
        File file = new File(this.activity.getFilesDir(), FILENAME);
        Files.write(file.toPath(), urls, StandardCharsets.UTF_8);
    }
}
