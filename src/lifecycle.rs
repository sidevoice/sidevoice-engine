/// Where a build is on its way to running: `Absent → Installing → Installed → Loading → Ready`, or `Failed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildState {
    Absent,
    Installing,
    Installed,
    Loading,
    Ready,
    Failed(crate::Error),
}
