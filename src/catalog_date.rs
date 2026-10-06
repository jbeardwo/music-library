//! Precision-aware comparative evidence, never Album or edition identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Date {
    pub year: u16,
    pub month: Option<u8>,
    pub day: Option<u8>,
}
impl Date {
    pub fn parse(value: &str) -> Option<Self> {
        let parts: Vec<_> = value.split('-').collect();
        if !(1..=3).contains(&parts.len())
            || parts[0].len() != 4
            || parts.iter().any(|p| !p.bytes().all(|b| b.is_ascii_digit()))
        {
            return None;
        }
        let year = parts[0].parse().ok()?;
        if year == 0 {
            return None;
        }
        let month = if parts.len() > 1 {
            let m = parts[1].parse().ok()?;
            if parts[1].len() != 2 || !(1..=12).contains(&m) {
                return None;
            }
            Some(m)
        } else {
            None
        };
        let day = if parts.len() > 2 {
            let d = parts[2].parse().ok()?;
            let max = match month? {
                2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
                2 => 28,
                4 | 6 | 9 | 11 => 30,
                _ => 31,
            };
            if parts[2].len() != 2 || !(1..=max).contains(&d) {
                return None;
            }
            Some(d)
        } else {
            None
        };
        Some(Self { year, month, day })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Agreement {
    UnknownOrDifferent,
    Year,
    Month,
    Day,
}
pub fn agreement(known: Option<Date>, candidate: Option<Date>) -> Agreement {
    let (Some(a), Some(b)) = (known, candidate) else {
        return Agreement::UnknownOrDifferent;
    };
    if a.year != b.year {
        return Agreement::UnknownOrDifferent;
    }
    match (a.month, b.month) {
        (Some(x), Some(y)) if x == y => match (a.day, b.day) {
            (Some(x), Some(y)) if x == y => Agreement::Day,
            _ => Agreement::Month,
        },
        _ => Agreement::Year,
    }
}
