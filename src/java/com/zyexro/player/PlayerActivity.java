package com.zyexro.player;

import android.app.NativeActivity;
import android.content.Intent;
import android.net.Uri;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;

/**
 * NativeActivity subclass that exists only to receive the SAF
 * (ACTION_OPEN_DOCUMENT) result. Pure-Rust NativeActivity cannot get
 * onActivityResult, so we forward it to Rust via a native callback.
 */
public class PlayerActivity extends NativeActivity {
    static {
        // Load the Rust cdylib so ART can link our `native` methods to it.
        System.loadLibrary("player");
    }

    private static final int REQUEST_OPEN_DOCUMENT = 42;
    private static final int READ_BUFFER = 8192;

    // Called from Rust (via JNI) to open the system audio file picker.
    public void launchAudioPicker() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("audio/*");
        startActivityForResult(intent, REQUEST_OPEN_DOCUMENT);
    }

    // Called from Rust (via JNI) on a background thread; returns the file bytes.
    // The content URI already carries a temporary read grant from the picker.
    public byte[] readUriBytes(String uri) throws IOException {
        InputStream in = getContentResolver().openInputStream(Uri.parse(uri));
        if (in == null) {
            return null;
        }
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        try {
            byte[] buf = new byte[READ_BUFFER];
            int n;
            while ((n = in.read(buf)) != -1) {
                out.write(buf, 0, n);
            }
        } finally {
            in.close();
        }
        return out.toByteArray();
    }

    private native void onOpenDocumentResult(int resultCode, String uri);

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        if (requestCode == REQUEST_OPEN_DOCUMENT) {
            String uri = (data != null && data.getData() != null)
                    ? data.getData().toString()
                    : null;
            onOpenDocumentResult(resultCode, uri);
            return;
        }
        super.onActivityResult(requestCode, resultCode, data);
    }
}