use super::{MapFn, Widget};
use crate::reference::Source;

/// What a row of a [`Table`] stands for in the view: something of a
/// placed part (or of the part itself), drawn highlighted while the
/// pointer is on the row.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Pointed {
    /// The instances it is in, from the part being edited down; empty
    /// for something of that part itself.
    pub instance: Vec<String>,
    /// What it is, in the world of the last of those instances.
    pub source: Source,
}

/// A list of like things as rows under column headings — the bodies a
/// placed part brings, the mates that hold it — kept compact: past
/// [`shown`](Self::shown) rows it scrolls.
///
/// Each row is a [`Widget::HBox`]: its first [`columns`](Self::columns)
/// children are its cells; any after those are more of the row (a
/// mate's offset and limits), shown under it when it is opened. The
/// rows are the table's children, so the fields in them are found by
/// widget path as any other.
pub struct Table<M> {
    /// The headings of the columns; an empty one heads a column of
    /// buttons.
    pub columns: Vec<String>,
    /// The rows: each a box of its cells, then what it has besides.
    pub rows: Vec<Widget<M>>,
    /// What each row stands for in the view, if anything.
    pub pointed: Vec<Option<Pointed>>,
    /// How many rows show at once before it scrolls.
    pub shown: usize,
}

impl<M: 'static> Table<M> {
    /// A table with `columns`, and no rows yet; five rows show at once.
    pub fn new(columns: Vec<String>) -> Self {
        Self {
            columns,
            rows: Vec::new(),
            pointed: Vec::new(),
            shown: 5,
        }
    }

    /// The same table with a row more: `cells` (one for each column,
    /// then what the row has besides), standing for `pointed`.
    #[must_use]
    pub fn row(mut self, cells: Vec<Widget<M>>, pointed: Option<Pointed>) -> Self {
        self.rows.push(super::HBox::new(cells).into());
        self.pointed.push(pointed);
        self
    }

    /// The same table, showing `rows` rows at once.
    #[must_use]
    pub fn showing(mut self, rows: usize) -> Self {
        self.shown = rows.max(1);
        self
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> Table<N> {
        Table {
            columns: self.columns,
            rows: self.rows.into_iter().map(|w| w.map_rc(f)).collect(),
            pointed: self.pointed,
            shown: self.shown,
        }
    }
}
