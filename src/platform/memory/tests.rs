//! The decisions `MemoryObservation` makes from what an OS shows, against fake OSes that show each case, and this machine's
//! own.

use super::{MemoryObservation, Observer};

const MIB: u64 = 1024 * 1024;

/// An OS that shows what it is built with.
struct Fake<const PHYSICAL: u64, const LIMIT: u64, const FREE: u64, const PROCESS: u64>;

/// 0 stands for "cannot tell", except where a test needs a zero reading (`Zero*`).
const fn some(bytes: u64) -> Option<u64> {
    if bytes == 0 {
        None
    } else {
        Some(bytes)
    }
}

impl<const PHYSICAL: u64, const LIMIT: u64, const FREE: u64, const PROCESS: u64> Observer
    for Fake<PHYSICAL, LIMIT, FREE, PROCESS>
{
    fn physical_bytes() -> Option<u64> {
        some(PHYSICAL)
    }
    fn limit_bytes() -> Option<u64> {
        some(LIMIT)
    }
    fn free_bytes() -> Option<u64> {
        some(FREE)
    }
    fn process_bytes() -> Option<u64> {
        some(PROCESS)
    }
}

/// An OS whose machine has no memory left, and one that says zero for everything.
struct NothingFree;
impl Observer for NothingFree {
    fn physical_bytes() -> Option<u64> {
        Some(8 * 1024 * MIB)
    }
    fn limit_bytes() -> Option<u64> {
        None
    }
    fn free_bytes() -> Option<u64> {
        Some(0)
    }
}
struct AllZero;
impl Observer for AllZero {
    fn physical_bytes() -> Option<u64> {
        Some(0)
    }
    fn limit_bytes() -> Option<u64> {
        Some(0)
    }
    fn free_bytes() -> Option<u64> {
        Some(0)
    }
    fn process_bytes() -> Option<u64> {
        Some(0)
    }
}

#[test]
fn memory_is_reported_in_whole_mebibytes() {
    let memory =
        MemoryObservation::observed::<Fake<{ 8 * 1024 * MIB + 1 }, 0, { 3 * MIB + MIB / 2 }, 0>>();
    assert_eq!(memory.total_mb, Some(8192));
    assert_eq!(memory.available_mb, Some(3));
}

#[test]
fn the_total_is_the_lower_of_the_physical_memory_and_the_groups_limit() {
    let limited =
        MemoryObservation::observed::<Fake<{ 16 * 1024 * MIB }, { 2 * 1024 * MIB }, 0, 0>>();
    assert_eq!(limited.total_mb, Some(2048));
    let unlimited =
        MemoryObservation::observed::<Fake<{ 16 * 1024 * MIB }, { 32 * 1024 * MIB }, 0, 0>>();
    assert_eq!(unlimited.total_mb, Some(16384));
}

#[test]
fn what_is_available_is_the_processs_budget_where_the_os_has_one_and_the_machines_otherwise() {
    let budget = MemoryObservation::observed::<
        Fake<{ 8 * 1024 * MIB }, 0, { 6 * 1024 * MIB }, { 3 * 1024 * MIB }>,
    >();
    assert_eq!(budget.available_mb, Some(3072));
    let machine =
        MemoryObservation::observed::<Fake<{ 8 * 1024 * MIB }, 0, { 6 * 1024 * MIB }, 0>>();
    assert_eq!(machine.available_mb, Some(6144));
}

#[test]
fn nothing_free_is_an_answer_and_an_unknown_total_is_not() {
    let empty = MemoryObservation::observed::<NothingFree>();
    assert_eq!(empty.total_mb, Some(8192));
    assert_eq!(empty.available_mb, Some(0));
    let unknown = MemoryObservation::observed::<AllZero>();
    assert_eq!(unknown.total_mb, None);
    assert_eq!(unknown.available_mb, Some(0));
}

#[test]
fn nothing_known_is_all_unknown() {
    let none = MemoryObservation::observed::<Fake<0, 0, 0, 0>>();
    assert_eq!(none.total_mb, None);
    assert_eq!(none.available_mb, None);
}

#[test]
fn a_figure_beyond_what_a_u32_of_mebibytes_holds_saturates() {
    let huge = MemoryObservation::observed::<Fake<{ u64::MAX }, 0, 0, 0>>();
    assert_eq!(huge.total_mb, Some(u32::MAX));
}

#[test]
fn this_machine_reports_its_memory_and_what_is_available() {
    let memory = MemoryObservation::now();
    assert!(
        memory.total_mb.expect("the OS reports its memory") >= 256,
        "{memory:?}"
    );
    assert!(memory.available_mb.is_some(), "{memory:?}");
}
