use crate::datatypes::Int64Chunked;
use crate::export::chrono::NaiveDateTime;
use crate::prelude::*;
use crate::prelude::{DatetimeChunked, TimeUnit};
use polars_time::export::chrono::Datelike;
pub use polars_time::*;

pub fn in_nanoseconds_window(ndt: &NaiveDateTime) -> bool {
    // ~584 year around 1970
    !(ndt.year() > 2554 || ndt.year() < 1386)
}

pub fn date_range(
    name: &str,
    start: i64,
    stop: i64,
    every: Duration,
    closed: ClosedWindow,
    tu: TimeUnit,
) -> DatetimeChunked {
    Int64Chunked::new_vec(
        name,
        date_range_vec(start, stop, every, closed, tu.to_polars_time()),
    )
    .into_datetime(tu, None)
}

impl DataFrame {
    /// Upsample a DataFrame at a regular frequency.
    ///
    /// # Arguments
    /// * `by` - First group by these columns and then upsample for every group
    /// * `time_column` - Will be used to determine a date_range.
    ///                   Note that this column has to be sorted for the output to make sense.
    /// * `every` - interval will start 'every' duration
    /// * `offset` - change the start of the date_range by this offset.
    ///
    /// The `period` and `offset` arguments are created with
    /// the following string language:
    /// - 1ns   (1 nanosecond)
    /// - 1us   (1 microsecond)
    /// - 1ms   (1 millisecond)
    /// - 1s    (1 second)
    /// - 1m    (1 minute)
    /// - 1h    (1 hour)
    /// - 1d    (1 day)
    /// - 1w    (1 week)
    /// - 1mo   (1 calendar month)
    /// - 1y    (1 calendar year)
    /// - 1i    (1 index count)
    /// Or combine them:
    /// "3d12h4m25s" # 3 days, 12 hours, 4 minutes, and 25 seconds
    pub fn upsample<I: IntoVec<String>>(
        &self,
        by: I,
        time_column: &str,
        every: Duration,
        offset: Duration,
    ) -> Result<DataFrame> {
        let by = by.into_vec();
        self.upsample_impl(by, time_column, every, offset, false)
    }

    /// Upsample a DataFrame at a regular frequency.
    ///
    /// # Arguments
    /// * `by` - First group by these columns and then upsample for every group
    /// * `time_column` - Will be used to determine a date_range.
    ///                   Note that this column has to be sorted for the output to make sense.
    /// * `every` - interval will start 'every' duration
    /// * `offset` - change the start of the date_range by this offset.
    ///
    /// The `period` and `offset` arguments are created with
    /// the following string language:
    /// - 1ns   (1 nanosecond)
    /// - 1us   (1 microsecond)
    /// - 1ms   (1 millisecond)
    /// - 1s    (1 second)
    /// - 1m    (1 minute)
    /// - 1h    (1 hour)
    /// - 1d    (1 day)
    /// - 1w    (1 week)
    /// - 1mo   (1 calendar month)
    /// - 1y    (1 calendar year)
    /// - 1i    (1 index count)
    /// Or combine them:
    /// "3d12h4m25s" # 3 days, 12 hours, 4 minutes, and 25 seconds
    pub fn upsample_stable<I: IntoVec<String>>(
        &self,
        by: I,
        time_column: &str,
        every: Duration,
        offset: Duration,
    ) -> Result<DataFrame> {
        let by = by.into_vec();
        self.upsample_impl(by, time_column, every, offset, true)
    }

    fn upsample_impl(
        &self,
        by: Vec<String>,
        index_column: &str,
        every: Duration,
        offset: Duration,
        stable: bool,
    ) -> Result<DataFrame> {
        let s = self.column(index_column)?;
        if matches!(s.dtype(), DataType::Date) {
            let mut df = self.clone();
            df.try_apply(index_column, |s| {
                s.cast(&DataType::Datetime(TimeUnit::Milliseconds, None))
            })
            .unwrap();
            let mut out = df
                .upsample_impl(by, index_column, every, offset, stable)
                .unwrap();
            out.try_apply(index_column, |s| s.cast(&DataType::Date))
                .unwrap();
            Ok(out)
        } else if by.is_empty() {
            let index_column = self.column(index_column)?;
            self.upsample_single_impl(index_column, every, offset)
        } else {
            let gb = if stable {
                self.groupby_stable(by)
            } else {
                self.groupby(by)
            };
            gb?.par_apply(|df| df.upsample_impl(vec![], index_column, every, offset, false))
        }
    }

    fn upsample_single_impl(
        &self,
        index_column: &Series,
        every: Duration,
        offset: Duration,
    ) -> Result<DataFrame> {
        let index_col_name = index_column.name();

        use DataType::*;
        match index_column.dtype() {
            Datetime(tu, _) => {
                let s = index_column.cast(&DataType::Int64).unwrap();
                let ca = s.i64().unwrap();
                let first = ca.into_iter().flatten().next();
                let last = ca.into_iter().flatten().next_back();
                match (first, last) {
                    (Some(first), Some(last)) => {
                        let first = match tu {
                            TimeUnit::Milliseconds => offset.add_ms(first),
                            TimeUnit::Nanoseconds => offset.add_ns(first),
                        };
                        let range =
                            date_range(index_col_name, first, last, every, ClosedWindow::Both, *tu)
                                .into_series()
                                .into_frame();
                        range.join(
                            self,
                            &[index_col_name],
                            &[index_col_name],
                            JoinType::Left,
                            None,
                        )
                    }
                    _ => Err(PolarsError::ComputeError(
                        "Cannot determine upsample boundaries. All elements are null.".into(),
                    )),
                }
            }
            dt => Err(PolarsError::ComputeError(
                format!("upsample not allowed for index_column of dtype {:?}", dt).into(),
            )),
        }
    }
}
