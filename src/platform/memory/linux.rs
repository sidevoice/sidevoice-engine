/// Linux: the machine's memory and its control group's limit, from `sysinfo` (`/proc/meminfo`, `/sys/fs/cgroup`).
pub struct Os;

impl super::Observer for Os {}
