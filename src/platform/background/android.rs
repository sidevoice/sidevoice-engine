/// Android: not seen to fail; sidevoice-web#572 asks to check a GPU build in the background before saying otherwise.
pub struct Os;

impl super::Background for Os {
    const GPU_STOPS_IN_BACKGROUND: bool = false;
}
