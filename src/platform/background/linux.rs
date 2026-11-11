/// Linux: a desktop keeps the GPU for an app whose window is hidden or minimised.
pub struct Os;

impl super::Background for Os {
    const GPU_STOPS_IN_BACKGROUND: bool = false;
}
