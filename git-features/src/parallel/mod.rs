//! Run computations in parallel, or not based the `parallel` feature toggle.
#[cfg(feature = "parallel")]
mod in_parallel;
mod serial;

#[cfg(not(feature = "parallel"))]
pub use serial::*;

#[cfg(feature = "parallel")]
pub use in_parallel::*;

mod eager;
pub use eager::{EagerIter, EagerIterIf};

/// A no-op returning the input _(`desired_chunk_size`, `Some(thread_limit)`, `thread_limit)_ used
/// when the `parallel` feature toggle is not set.
#[cfg(not(feature = "parallel"))]
#[must_use]
pub fn optimize_chunk_size_and_thread_limit(
    desired_chunk_size: usize,
    _num_items: Option<usize>,
    thread_limit: Option<usize>,
    _available_threads: Option<usize>,
) -> (usize, Option<usize>, usize) {
    (desired_chunk_size, thread_limit, num_threads(thread_limit))
}

/// Return the 'optimal' _(`size of chunks`,  `amount of threads as Option`, `amount of threads`)_ to use in [`in_parallel()`] for the given
/// `desired_chunk_size`, `num_items`, `thread_limit` and `available_threads`.
///
/// * `desired_chunk_size` is the amount of items per chunk you think should be used.
/// * `num_items` is the total amount of items in the iteration, if `Some`.
///    Otherwise this knowledge will not affect the output of this function.
/// * `thread_limit` is the amount of threads to use at most, if `Some`.
///    Otherwise this knowledge will not affect the output of this function.
/// * `available_threads` is the total amount of threads available, if `Some`.
///    Otherwise the actual amount of available threads is determined by querying the system.
///
/// `Note` that this implementation is available only if the `parallel` feature toggle is set.
#[cfg(feature = "parallel")]
pub fn optimize_chunk_size_and_thread_limit(
    desired_chunk_size: usize,
    num_items: Option<usize>,
    thread_limit: Option<usize>,
    available_threads: Option<usize>,
) -> (usize, Option<usize>, usize) {
    let available_threads = available_threads.unwrap_or_else(num_cpus::get);
    let available_threads = thread_limit
        .map(|l| if l == 0 { available_threads } else { l })
        .unwrap_or(available_threads);

    let (lower, upper) = (50, 1000);
    let (chunk_size, thread_limit) = num_items
        .map(|num_items| {
            let desired_chunks_per_thread_at_least = 2;
            let items = num_items;
            let chunk_size = (items / (available_threads * desired_chunks_per_thread_at_least))
                .max(1)
                .min(upper);
            let num_chunks = items / chunk_size;
            let thread_limit = if num_chunks <= available_threads {
                (num_chunks / desired_chunks_per_thread_at_least).max(1)
            } else {
                available_threads
            };
            (chunk_size, thread_limit)
        })
        .unwrap_or({
            let chunk_size = if available_threads == 1 {
                desired_chunk_size
            } else if desired_chunk_size < lower {
                lower
            } else {
                desired_chunk_size.min(upper)
            };
            (chunk_size, available_threads)
        });
    (chunk_size, Some(thread_limit), thread_limit)
}

/// Always returns 1, available when the `parallel` feature toggle is unset.
#[cfg(not(feature = "parallel"))]
pub(crate) fn num_threads(_thread_limit: Option<usize>) -> usize {
    1
}

/// Returns the amount of threads the system can effectively use as the amount of its logical cores.
///
/// Only available with the `parallel` feature toggle set.
#[cfg(feature = "parallel")]
pub(crate) fn num_threads(thread_limit: Option<usize>) -> usize {
    let logical_cores = num_cpus::get();
    thread_limit
        .map(|l| if l == 0 { logical_cores } else { l })
        .unwrap_or(logical_cores)
}

/// An trait for aggregating items into a single result.
pub trait Reducer {
    /// The type fed to the reducer in the [`feed()`][`Reducer::feed()`] method.
    type Input;
    /// The type produced once by the [`finalize()`][`Reducer::finalize()`] method.
    type Output;
    /// The error type to use for all methods of this trait.
    type Error;
    /// Called each time a new `item` was produced in order to aggregate it into the final result.
    ///
    /// If an `Error` is returned, the entire operation will be stopped.
    fn feed(&mut self, item: Self::Input) -> Result<(), Self::Error>;
    /// Called once once all items where passed to `feed()`, producing the final `Output` of the operation or an `Error`.
    fn finalize(self) -> Result<Self::Output, Self::Error>;
}

/// Run [`in_parallel()`] only if the given `condition()` returns true when eagerly evaluated.
///
/// For parameters, see the documentation of [`in_parallel()`]
pub fn in_parallel_if<I, S, O, R>(
    condition: impl FnOnce() -> bool,
    input: impl Iterator<Item = I> + Send,
    thread_limit: Option<usize>,
    new_thread_state: impl Fn(usize) -> S + Send + Sync,
    consume: impl Fn(I, &mut S) -> O + Send + Sync,
    reducer: R,
) -> Result<<R as Reducer>::Output, <R as Reducer>::Error>
where
    R: Reducer<Input = O>,
    I: Send,
    O: Send,
{
    if num_threads(thread_limit) > 1 && condition() {
        in_parallel(input, thread_limit, new_thread_state, consume, reducer)
    } else {
        serial::in_parallel(input, thread_limit, new_thread_state, consume, reducer)
    }
}
