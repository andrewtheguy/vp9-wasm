//! Running a frame's independent parts side by side on rayon's pool, or one
//! after another without it.

use crate::error::Result;

/// Runs `job` on every item: on the pool when there are threads and more
/// than one item, else in order on the caller. The first error, in the items'
/// order, is the one returned.
pub(crate) fn each<T: Send>(items: &mut [T], threads: usize, job: impl Fn(&mut T) -> Result<()> + Sync) -> Result<()> {
    #[cfg(feature = "threads")]
    if threads > 1 && items.len() > 1 {
        use rayon::prelude::*;
        let results: Vec<Result<()>> = items.par_iter_mut().map(&job).collect();
        return results.into_iter().collect();
    }
    let _ = threads;
    items.iter_mut().try_for_each(job)
}
