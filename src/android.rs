//! Android-only glue: bridges a Java `NativeActivity` subclass (SAF file picker)
//! to the Rust UI via a channel, and streams picked audio bytes to rodio.
//!
//! :ponytail: We read whole files into memory (fine for songs, not for long
//! audiobooks/podcasts). CEILING: streaming. UPGRADE: relay `InputStream` chunks
//! to a growing buffer and hand `rodio` a streaming `Read`, or switch to
//! ExoPlayer/Media3 via `jni`.

use jni::{
    EnvUnowned,
    errors::{Error, ThrowRuntimeExAndDefault},
    jni_sig, jni_str,
    objects::{JByteArray, JObject, JString, Reference},
    sys,
};
use std::sync::{
    Mutex, OnceLock, mpsc,
};

/// Must match `PlayerActivity.REQUEST_OPEN_DOCUMENT` (Java side).
pub const REQUEST_OPEN_DOCUMENT: sys::jint = 42;
const RESULT_OK: sys::jint = -1;

/// Messages delivered to the UI thread from the Java/IO side.
pub enum Msg {
    /// The user picked a document in the SAF picker.
    Picked { uri: String },
    /// `load_async` finished reading the file.
    Loaded { bytes: Vec<u8>, name: String },
    Error(String),
}

static CHANNEL: OnceLock<(mpsc::Sender<Msg>, Mutex<mpsc::Receiver<Msg>>)> = OnceLock::new();

fn channel() -> &'static (mpsc::Sender<Msg>, Mutex<mpsc::Receiver<Msg>>) {
    CHANNEL.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        (tx, Mutex::new(rx))
    })
}

fn send(msg: Msg) {
    let _ = channel().0.send(msg);
}

/// Pop at most one pending message; call repeatedly to drain.
pub fn poll() -> Option<Msg> {
    channel().1.lock().ok()?.try_recv().ok()
}

/// Launch the system `ACTION_OPEN_DOCUMENT` picker (audio MIME only).
pub fn pick_audio() {
    if let Err(e) = launch_picker() {
        send(Msg::Error(format!("launchAudioPicker: {e}")));
    }
}

fn launch_picker() -> jni::errors::Result<()> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    let mut scope = jni::ScopeToken::default();
    let mut guard = unsafe { vm.attach_current_thread_guard(Default::default, &mut scope)? };
    let env = guard.borrow_env_mut();
    let context = unsafe { JObject::from_raw(env, ctx.context().cast()) };
    env.call_method(
        &context,
        jni_str!("launchAudioPicker"),
        jni_sig!(() -> void),
        &[],
    )?;
    Ok(())
}

/// Spawn a background read of the picked content URI; the result comes back via
/// [`poll`] as `Msg::Loaded` / `Msg::Error`.
pub fn load_async(uri: String, name: String) {
    std::thread::spawn(move || match read_all_bytes(&uri) {
        Ok(bytes) => send(Msg::Loaded { bytes, name }),
        Err(e) => send(Msg::Error(format!("read {name}: {e}"))),
    });
}

/// Read a `content://` URI to bytes, delegating the actual IO to the Java shim
/// to keep the JNI surface small.
fn read_all_bytes(uri: &str) -> jni::errors::Result<Vec<u8>> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    let mut scope = jni::ScopeToken::default();
    let mut guard = unsafe { vm.attach_current_thread_guard(Default::default, &mut scope)? };
    let env = guard.borrow_env_mut();
    let context = unsafe { JObject::from_raw(env, ctx.context().cast()) };
    let j_uri = env.new_string(uri)?;
    let bytes_obj = env
        .call_method(
            &context,
            jni_str!("readUriBytes"),
            jni_sig!((java.lang.String) -> [jbyte]),
            &[(&j_uri).into()],
        )?
        .l()?;
    if bytes_obj.is_null() {
        return Err(Error::NullPtr("readUriBytes returned null"));
    }
    let jarr = unsafe { JByteArray::from_raw(env, bytes_obj.as_raw()) };
    env.convert_byte_array(jarr)
}

// --- Native callbacks invoked from PlayerActivity on the Java main thread ---

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn Java_com_zyexro_player_PlayerActivity_onOpenDocumentResult<'local>(
    mut env: EnvUnowned<'local>,
    _this: JObject<'local>,
    result_code: sys::jint,
    uri: JString<'local>,
) {
    if result_code != RESULT_OK || uri.is_null() {
        return;
    }
    env.with_env(|env| -> jni::errors::Result<()> {
        let uri = uri.try_to_string(env)?;
        send(Msg::Picked { uri });
        Ok(())
    })
    .resolve::<ThrowRuntimeExAndDefault>();
}