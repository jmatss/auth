import android.R;
import android.app.Activity;
import android.app.AlertDialog;
import android.content.DialogInterface;

/**
 * Helper to show dialogs.
 */
public class DialogHelper {
    private final Activity activity;

    public DialogHelper(Activity activity) {
        this.activity = activity;
    }

    public void showError(String title, String message) {
        AlertDialog.Builder builder = new AlertDialog.Builder(activity, R.style.Theme_DeviceDefault_Dialog_Alert);

        if (title != null) {
            builder.setTitle(title);
        }

        builder.setMessage(message)
                .setNeutralButton(R.string.ok, new DialogInterface.OnClickListener() {
                    public void onClick(DialogInterface dialog, int id) {
                        // Do nothing, just an OK click that should close the dialog.
                    }
                });

        this.activity.runOnUiThread(() -> builder.create().show());
    }
}
