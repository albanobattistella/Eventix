use std::{io, path::Path};

use chrono::NaiveDate;
use chrono_tz::Tz;
use eventix_ical::objects::CalLocale;

use crate::{DateFlags, DateLike, LocaleType, Translations};

use super::Locale;

/// Implementazione della locale italiana.
///
/// Traduce nomi di giorni e mesi tramite la tabella delle traduzioni.
/// Usa formati data italiani:
/// - breve: `DD MMM YYYY`
/// - lungo: `dddd DD MMMM YYYY`
#[derive(Default, Debug)]
pub struct LocaleIt {
    tz: Tz,
    trans: Translations,
}

impl LocaleIt {
    /// Crea una nuova `LocaleIt` con il fuso orario specificato,
    /// caricando le traduzioni dal file indicato.
    pub(crate) fn new(tz: Tz, path: &Path) -> io::Result<Self> {
        let trans = Translations::new_from_file(path)?;
        Ok(Self { tz, trans })
    }
}

impl Locale for LocaleIt {
    fn ty(&self) -> LocaleType {
        LocaleType::Italian
    }

    fn translations(&self) -> &Translations {
        &self.trans
    }

    fn fmt_weekdate(&self, date: &dyn DateLike, flags: DateFlags) -> String {
        if !flags.contains(DateFlags::NoToday)
            && let Some(rel) = self.has_relative(date)
        {
            return rel.to_string();
        }

        // Giorno della settimana
        let wday_fmt = if flags.contains(DateFlags::Short) { "%a" } else { "%A" };
        let wday_it = self.translate(&date.fmt(wday_fmt));

        // Mese abbreviato
        let mon_it = self.translate(&date.fmt("%b"));

        let fmt = if flags.contains(DateFlags::Short) {
            "%d"
        } else {
            "%d %Y"
        };

        format!("{}, {} {}", wday_it, mon_it, date.fmt(fmt))
    }

    fn fmt_date(&self, date: &dyn DateLike, flags: DateFlags) -> String {
        if !flags.contains(DateFlags::NoToday)
            && let Some(rel) = self.has_relative(date)
        {
            return rel.to_string();
        }

        // Giorno della settimana (solo formato lungo)
        let wday = if !flags.contains(DateFlags::Short) {
            let wday_it = self.translate(&date.fmt("%A"));
            format!("{}, ", wday_it)
        } else {
            String::new()
        };

        // Mese (abbrev. o completo)
        let mon_fmt = if flags.contains(DateFlags::Short) { "%b" } else { "%B" };
        let mon_it = self.translate(&date.fmt(mon_fmt));

        let day_year = date.fmt("%d, %Y");

        format!("{}{} {}", wday, mon_it, day_year)
    }
}

impl CalLocale for LocaleIt {
    fn translate<'a>(&'a self, key: &'a str) -> &'a str {
        self.translations().table.get(key).map_or(key, |v| v)
    }

    fn timezone(&self) -> &Tz {
        &self.tz
    }

    /// Ordinali italiani:
    /// 1 → "primo", 2 → "secondo", 3 → "terzo", 4+ → "n-esimo"
    /// Per la parte finale:
    /// 1 → "ultimo", 2 → "penultimo", 3+ → "n-esimo dall'ultimo"
    fn nth_day(&self, nth: u64, start: bool) -> String {
        match start {
            true => match nth {
                1 => "primo".into(),
                2 => "secondo".into(),
                3 => "terzo".into(),
                n => format!("{n}esimo"),
            },
            false => match nth {
                1 => "ultimo".into(),
                2 => "penultimo".into(),
                n => format!("{n}esimo dall'ultimo"),
            },
        }
    }

    fn fmt_naive_date(&self, date: &NaiveDate) -> String {
        self.fmt_date(date, DateFlags::Short)
    }
}
