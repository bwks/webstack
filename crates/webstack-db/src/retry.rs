use std::{
    future::Future,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use surrealdb::types::{ErrorDetails, QueryError};

const RETRY_WINDOWS_MS: [(u64, u64); 3] = [(10, 50), (20, 100), (40, 200)];

static JITTER_STATE: AtomicU64 = AtomicU64::new(0);

/// Repeats a transaction-safe write after retryable transaction conflicts.
///
/// The closure runs once initially and at most three more times. Keep external
/// side effects outside the closure because any attempted operation may run
/// more than once.
///
/// # Errors
///
/// Returns non-conflict errors immediately and the final conflict after the
/// retry budget is exhausted.
pub async fn retry_write<T, F, Fut>(operation: F) -> surrealdb::Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = surrealdb::Result<T>>,
{
    let delays = RETRY_WINDOWS_MS
        .map(|(minimum, maximum)| Duration::from_millis(jittered_milliseconds(minimum, maximum)));
    retry_write_with_delays(operation, &delays, |delay| tokio::time::sleep(delay)).await
}

/// Classifies the retryable conflict shape exposed by `SurrealDB` 3.2.1.
fn is_retryable_transaction_conflict(error: &surrealdb::Error) -> bool {
    let typed_conflict = matches!(
        error.details(),
        ErrorDetails::Query(Some(QueryError::TransactionConflict))
    );
    let compatibility_conflict = error.kind_str() == "Internal"
        && error.message().starts_with("Transaction conflict:")
        && error.message().ends_with("This transaction can be retried");
    typed_conflict || compatibility_conflict
}

/// Retries using caller-supplied deterministic delays and delay implementation.
async fn retry_write_with_delays<T, F, Fut, D, DelayFuture>(
    mut operation: F,
    delays: &[Duration],
    mut delay: D,
) -> surrealdb::Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = surrealdb::Result<T>>,
    D: FnMut(Duration) -> DelayFuture,
    DelayFuture: Future<Output = ()>,
{
    for retry_delay in delays {
        match operation().await {
            Err(error) if is_retryable_transaction_conflict(&error) => delay(*retry_delay).await,
            result => return result,
        }
    }
    operation().await
}

/// Selects a lightweight pseudo-random millisecond value within an inclusive window.
fn jittered_milliseconds(minimum: u64, maximum: u64) -> u64 {
    let seed = JITTER_STATE.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed)
        ^ SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| {
                duration.as_secs() ^ u64::from(duration.subsec_nanos())
            });
    minimum + seed % (maximum - minimum + 1)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use surrealdb::types::{ErrorDetails, QueryError};

    use super::{RETRY_WINDOWS_MS, is_retryable_transaction_conflict, retry_write_with_delays};

    #[test]
    fn surrealdb_conflict_compatibility_is_narrow() {
        let typed = surrealdb::Error::query(
            "Transaction conflict".to_owned(),
            QueryError::TransactionConflict,
        );
        let internal = surrealdb::Error::internal(
            "Transaction conflict: key. This transaction can be retried".to_owned(),
        );
        let unrelated = surrealdb::Error::from_details(
            "Transaction conflict: key".to_owned(),
            ErrorDetails::Internal,
        );
        assert!(is_retryable_transaction_conflict(&typed));
        assert!(is_retryable_transaction_conflict(&internal));
        assert!(!is_retryable_transaction_conflict(&unrelated));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retry_policy_uses_three_injected_delays_then_returns_final_conflict() {
        let attempts = Rc::new(RefCell::new(0));
        let observed = Rc::new(RefCell::new(Vec::new()));
        let expected = [
            std::time::Duration::from_millis(10),
            std::time::Duration::from_millis(20),
            std::time::Duration::from_millis(40),
        ];
        let result = retry_write_with_delays(
            {
                let attempts = Rc::clone(&attempts);
                move || {
                    *attempts.borrow_mut() += 1;
                    async {
                        Err::<(), _>(surrealdb::Error::query(
                            "Transaction conflict".to_owned(),
                            QueryError::TransactionConflict,
                        ))
                    }
                }
            },
            &expected,
            {
                let observed = Rc::clone(&observed);
                move |duration| {
                    observed.borrow_mut().push(duration);
                    async {}
                }
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(*attempts.borrow(), 4);
        assert_eq!(*observed.borrow(), expected);
        assert_eq!(RETRY_WINDOWS_MS, [(10, 50), (20, 100), (40, 200)]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retry_succeeds_after_conflicts_and_stops_on_other_errors() {
        let attempts = Rc::new(RefCell::new(0));
        let result = retry_write_with_delays(
            {
                let attempts = Rc::clone(&attempts);
                move || {
                    *attempts.borrow_mut() += 1;
                    let attempt = *attempts.borrow();
                    async move {
                        if attempt < 3 {
                            Err(surrealdb::Error::query(
                                "Transaction conflict".to_owned(),
                                QueryError::TransactionConflict,
                            ))
                        } else {
                            Ok(7)
                        }
                    }
                }
            },
            &[std::time::Duration::ZERO; 3],
            |_| async {},
        )
        .await;
        assert_eq!(result.expect("eventual success"), 7);
        assert_eq!(*attempts.borrow(), 3);

        let attempts = Rc::new(RefCell::new(0));
        let result = retry_write_with_delays(
            {
                let attempts = Rc::clone(&attempts);
                move || {
                    *attempts.borrow_mut() += 1;
                    async { Err::<(), _>(surrealdb::Error::internal("not retryable".to_owned())) }
                }
            },
            &[std::time::Duration::ZERO; 3],
            |_| async {},
        )
        .await;
        assert!(result.is_err());
        assert_eq!(*attempts.borrow(), 1);
    }
}
