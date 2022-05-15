//! # Twilight Bucket
//! A utility crate to limit users' usage, a third party crate of the
//! [Twilight ecosystem](https://docs.rs/twilight)
//!
//! All the functionality of this crate is under [`Bucket`], see its
//! documentation for usage info
//!
//! This crate can be used with any library, but it shares Twilight's non-goals,
//! such as trying to be more verbose and less opinionated and
//! [Serenity already has a bucket implementation
//! ](https://docs.rs/serenity/latest/serenity/framework/standard/buckets)
//!
//! # Example
//! ```
//! use std::{num::NonZeroU64, time::Duration};
//!
//! use twilight_bucket::{Bucket, Limit};
//!
//! #[tokio::main]
//! async fn main() {
//!     // A user can use it once every 10 seconds
//!     let my_command_user_bucket = Bucket::new(Limit::new(Duration::from_secs(10), 1));
//!     // It can be used up to 5 times every 30 seconds in one channel
//!     let my_command_channel_bucket = Bucket::new(Limit::new(Duration::from_secs(30), 5));
//!     run_my_command(
//!         my_command_user_bucket,
//!         my_command_channel_bucket,
//!         12345,
//!         123,
//!     )
//!     .await;
//! }
//!
//! async fn run_my_command(
//!     user_bucket: Bucket,
//!     channel_bucket: Bucket,
//!     user_id: u64,
//!     channel_id: u64,
//! ) -> String {
//!     if let Some(channel_limit_duration) = channel_bucket.limit_duration(channel_id) {
//!         return format!(
//!             "This was used too much in this channel, please wait {} seconds",
//!             channel_limit_duration.as_secs()
//!         );
//!     }
//!     if let Some(user_limit_duration) = user_bucket.limit_duration(user_id) {
//!         if Duration::from_secs(5) > user_limit_duration {
//!             tokio::time::sleep(user_limit_duration).await;
//!         } else {
//!             return format!(
//!                 "You've been using this too much, please wait {} seconds",
//!                 user_limit_duration.as_secs()
//!             );
//!         }
//!     }
//!     user_bucket.register(user_id);
//!     channel_bucket.register(channel_id);
//!     "Ran your command".to_owned()
//! }
//! ```

#![warn(clippy::cargo, clippy::nursery, clippy::pedantic, clippy::restriction)]
#![allow(
    clippy::blanket_clippy_restriction_lints,
    clippy::missing_inline_in_public_items,
    clippy::implicit_return,
    clippy::shadow_same,
    clippy::separated_literal_suffix
)]

use std::{
    num::NonZeroU64,
    time::{Duration, Instant},
};

use dashmap::DashMap;

/// Information about how often something is able to be used
///
/// # examples
/// Something can be used every 3 seconds
/// ```
/// twilight_bucket::Limit::new(std::time::Duration::from_secs(3), 1);
/// ```
/// Something can be used 10 times in 1 minute, so the limit resets every minute
/// ```
/// twilight_bucket::Limit::new(std::time::Duration::from_secs(60), 10);
/// ```
#[must_use]
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct Limit {
    /// How often something can be done [`Limit::count`] times
    duration: Duration,
    /// How many times something can be done in the [`Limit::duration`] period
    count: u16,
}

impl Limit {
    /// Create a new [`Limit`]
    pub const fn new(duration: Duration, count: u16) -> Self {
        Self { duration, count }
    }
}

/// Usage information about an ID
#[must_use]
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
struct Usage {
    /// The last time it was used
    time: Instant,
    /// How many times it was used
    count: u16,
}

impl Usage {
    /// Make a `Usage` with now as `time` and 1 as `count`
    fn new() -> Self {
        Self {
            time: Instant::now(),
            count: 1,
        }
    }
}

/// This is the main struct to do everything you need
///
/// # Global or task-based
/// Essentially buckets just store usages and limits, meaning you can create a
/// different bucket for each kind of limit: each of your commands, separate
/// buckets for channel and user usage if you want to have different limits for
/// each etc.
///
/// # Usage
/// Register usages using the [`Bucket::register`] method **after** getting the
/// limit with [`Bucket::limit_duration`]
///
/// `ID`s use [`NonZeroU64`](std::num::NonZeroU64) to be compatible with any
/// kind of ID: users, guilds or even your custom IDs
#[must_use]
#[derive(Debug)]
pub struct Bucket {
    /// The limit for this bucket
    limit: Limit,
    /// Usage information for IDs
    usages: DashMap<NonZeroU64, Usage>,
}

impl Bucket {
    /// Create a new [`Bucket`] with the given limit
    pub fn new(limit: Limit) -> Self {
        Self {
            limit,
            usages: DashMap::new(),
        }
    }

    /// Register a usage, you should call this every time something you want to
    /// limit is done **after** waiting for the limit
    ///
    /// ```
    /// # use std::time::Duration;
    /// # use twilight_bucket::{Bucket, Limit};
    /// # #[tokio::main]
    /// # async fn main() {
    /// # let user_id = 123;
    /// # let bucket = Bucket::new(Limit::new(Duration::from_secs(1), 1));
    /// if let Some(duration) = bucket.limit_duration(user_id) {
    ///     tokio::time::sleep(duration).await;
    /// }
    /// bucket.register(user_id);
    /// # }
    /// ```
    ///
    /// # Panics
    /// If the `id` is 0 or when the usage count is over [`u16::MAX`]
    #[allow(clippy::unwrap_used, clippy::integer_arithmetic)]
    pub fn register(&self, id: u64) {
        let id_non_zero = id.try_into().unwrap();
        match self.usages.get_mut(&id_non_zero) {
            Some(mut usage) => {
                let now = Instant::now();
                usage.count = if now - usage.time > self.limit.duration {
                    1
                } else {
                    usage.count + 1
                };
                usage.time = now;
            }
            None => {
                self.usages.insert(id_non_zero, Usage::new());
            }
        }
    }

    /// Get the duration to wait until the next usage by `id`, returns `None`
    /// if the `id` isn't limited, you should call this **before** registering a
    /// usage
    ///
    /// ```
    /// # use std::time::Duration;
    /// # use twilight_bucket::{Bucket, Limit};
    /// # #[tokio::main]
    /// # async fn main() {
    /// # let user_id = 123;
    /// # let bucket = Bucket::new(Limit::new(Duration::from_secs(1), 1));
    /// if let Some(duration) = bucket.limit_duration(user_id) {
    ///     tokio::time::sleep(duration).await;
    /// }
    /// bucket.register(user_id);
    /// # }
    /// ```
    ///
    /// # Panics
    /// If the `id` is 0
    #[must_use]
    #[allow(clippy::unwrap_in_result, clippy::unwrap_used)]
    pub fn limit_duration(&self, id: u64) -> Option<Duration> {
        let usage = self.usages.get(&id.try_into().unwrap())?;
        let elapsed = Instant::now() - usage.time;
        (usage.count >= self.limit.count && self.limit.duration > elapsed)
            .then(|| self.limit.duration - elapsed)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::sleep;

    use crate::{Bucket, Limit};

    #[allow(clippy::unwrap_used)]
    #[tokio::test]
    async fn limit_count_1() {
        let bucket = Bucket::new(Limit::new(Duration::from_secs(2), 1));
        let id = 123;

        assert!(bucket.limit_duration(id).is_none());

        bucket.register(id);
        assert!(
            bucket.limit_duration(id).unwrap()
                > bucket.limit.duration - Duration::from_secs_f32(0.1)
        );
        sleep(bucket.limit.duration).await;
        assert!(bucket.limit_duration(id).is_none());
    }

    #[allow(clippy::unwrap_used)]
    #[tokio::test]
    async fn limit_count_5() {
        let bucket = Bucket::new(Limit::new(Duration::from_secs(5), 5));
        let id = 123;

        for _ in 0_u8..5 {
            assert!(bucket.limit_duration(id).is_none());
            bucket.register(id);
        }

        assert!(
            bucket.limit_duration(id).unwrap()
                > bucket.limit.duration - Duration::from_secs_f32(0.1)
        );
        sleep(bucket.limit.duration).await;
        assert!(bucket.limit_duration(id).is_none());
    }
}
