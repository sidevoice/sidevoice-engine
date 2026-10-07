//! `Send` and `Sync` where the engine can run on several threads, and nothing where it cannot. A native host may move
//! the engine across threads, so everything the engine holds must be `Send + Sync` there; a page has one thread, and
//! what JavaScript hands the engine (a `JsValue`, a promise) is neither. The engine's traits take these as supertraits
//! instead of `Send + Sync`, so the same trait fits both.

/// `Send` in a native build; every type in the web build.
#[cfg(native)]
pub trait MaybeSend: Send {}
#[cfg(native)]
impl<T: ?Sized + Send> MaybeSend for T {}

/// `Send` in a native build; every type in the web build.
#[cfg(web)]
pub trait MaybeSend {}
#[cfg(web)]
impl<T: ?Sized> MaybeSend for T {}

/// `Sync` in a native build; every type in the web build.
#[cfg(native)]
pub trait MaybeSync: Sync {}
#[cfg(native)]
impl<T: ?Sized + Sync> MaybeSync for T {}

/// `Sync` in a native build; every type in the web build.
#[cfg(web)]
pub trait MaybeSync {}
#[cfg(web)]
impl<T: ?Sized> MaybeSync for T {}
