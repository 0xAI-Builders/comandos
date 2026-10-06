//! Escrituras en dos formatos con una transacción propia bajo el mismo flock.
use crate::{
    Error, Result,
    unified::{Mode, Origin},
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
pub(crate) fn write(
    mode: Mode,
    db: Option<&Connection>,
    mut file: impl FnMut() -> Result<()>,
    row: impl FnOnce(&Connection, Origin) -> Result<()>,
) -> Result<()> {
    if mode == Mode::Legacy {
        return file();
    }
    let Some(db) = db else {
        if mode == Mode::Sealed {
            return Err(Error::Validation(
                "dominio sellado y base no disponible".into(),
            ));
        }
        return file();
    };
    if !db.is_autocommit() {
        return Err(Error::Validation(
            "escritura de dominio requiere transacción propia para confirmar antes del espejo"
                .into(),
        ));
    }
    if mode == Mode::Mirror {
        file()?;
    }
    let tx = Transaction::new_unchecked(db, TransactionBehavior::Immediate)?;
    row(
        db,
        if mode == Mode::Mirror {
            Origin::Mirror
        } else {
            Origin::Unified
        },
    )?;
    tx.commit()?;
    if mode == Mode::Unified {
        file()?;
    }
    Ok(())
}
