/// iOS: a Metal command buffer committed while the app is not in the foreground fails (whisper.cpp then aborts).
pub struct Os;

impl super::Background for Os {
    const GPU_STOPS_IN_BACKGROUND: bool = true;
}
