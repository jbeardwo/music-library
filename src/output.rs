//! Fixed source calibration. No Track analysis, normalization or audio mutation.
use crate::{
    Library,
    playback::Volume,
    storage::{Error, Result},
};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OutputTrims {
    pub local_db: f64,
    pub spotify_db: f64,
}

impl OutputTrims {
    pub fn validate(self) -> Result<Self> {
        if [self.local_db, self.spotify_db]
            .into_iter()
            .all(|db| db.is_finite() && (-24.0..=0.0).contains(&db))
        {
            Ok(self)
        } else {
            Err(Error::Invalid(
                "Output trim must be between −24 and 0 dB".into(),
            ))
        }
    }
}

/// Convert a logical master level into bounded backend gain.
pub fn effective(master: Volume, db: f64) -> Volume {
    Volume::new((master.get() * 10_f64.powf(db / 20.)).clamp(0., 1.)).expect("validated gain")
}

impl Library {
    pub fn output_trims(&self) -> Result<OutputTrims> {
        self.store
            .connection
            .query_row(
                "SELECT local_db,spotify_db FROM output_calibration WHERE id=1",
                [],
                |r| {
                    Ok(OutputTrims {
                        local_db: r.get(0)?,
                        spotify_db: r.get(1)?,
                    })
                },
            )
            .map_err(Into::into)
    }
    pub fn set_output_trims(&mut self, trims: OutputTrims) -> Result<()> {
        let trims = trims.validate()?;
        self.store.connection.execute(
            "UPDATE output_calibration SET local_db=?1,spotify_db=?2 WHERE id=1",
            rusqlite::params![trims.local_db, trims.spotify_db],
        )?;
        Ok(())
    }
}
