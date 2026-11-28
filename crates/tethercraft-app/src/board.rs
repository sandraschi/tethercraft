//! Peer board: the RustDesk-style home of the controller window.
//!
//! Columns derive from address-book state, so the store stays the source of
//! truth and nothing is persisted here: Ready (address plus pinned host
//! key), First contact (address but no key yet - verify the words), Needs
//! address (nothing to dial yet). Filter matches id, alias, tier and address.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardCol {
    Ready,
    FirstContact,
    NeedsAddress,
}

impl BoardCol {
    pub const ALL: [BoardCol; 3] = [
        BoardCol::Ready,
        BoardCol::FirstContact,
        BoardCol::NeedsAddress,
    ];

    pub fn title(self) -> &'static str {
        match self {
            BoardCol::Ready => "Ready to connect",
            BoardCol::FirstContact => "First contact (verify words)",
            BoardCol::NeedsAddress => "Needs an address",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            BoardCol::Ready => "Address plus pinned key: double-click to start.",
            BoardCol::FirstContact => "Compare the host words out of band on first connect.",
            BoardCol::NeedsAddress => "Re-add the peer with --addr host:port.",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PeerCard {
    pub id: String,
    pub alias: String,
    pub addr: Option<String>,
    pub has_key: bool,
    pub tier: String,
}

impl PeerCard {
    pub fn display_name(&self) -> String {
        if self.alias.is_empty() {
            self.id.clone()
        } else {
            self.alias.clone()
        }
    }

    pub fn board_col(&self) -> BoardCol {
        match (&self.addr, self.has_key) {
            (Some(a), true) if !a.trim().is_empty() => BoardCol::Ready,
            (Some(a), _) if !a.trim().is_empty() => BoardCol::FirstContact,
            _ => BoardCol::NeedsAddress,
        }
    }

    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let q = query.to_lowercase();
        self.id.to_lowercase().contains(&q)
            || self.alias.to_lowercase().contains(&q)
            || self.tier.to_lowercase().contains(&q)
            || self
                .addr
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains(&q)
    }
}

#[derive(Debug, Default)]
pub struct BoardState {
    pub cards: Vec<PeerCard>,
    pub filter: String,
}

impl BoardState {
    pub fn new(cards: Vec<PeerCard>) -> Self {
        Self {
            cards,
            filter: String::new(),
        }
    }

    /// Visible cards for one column, filtered and sorted by display name.
    pub fn visible_in(&self, col: BoardCol) -> Vec<&PeerCard> {
        let mut out: Vec<&PeerCard> = self
            .cards
            .iter()
            .filter(|c| c.board_col() == col && c.matches(&self.filter))
            .collect();
        out.sort_by_key(|a| a.display_name());
        out
    }

    pub fn count_in(&self, col: BoardCol) -> usize {
        self.cards
            .iter()
            .filter(|c| c.board_col() == col && c.matches(&self.filter))
            .count()
    }

    pub fn total_shown(&self) -> usize {
        self.cards
            .iter()
            .filter(|c| c.matches(&self.filter))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, alias: &str, addr: Option<&str>, has_key: bool, tier: &str) -> PeerCard {
        PeerCard {
            id: id.to_string(),
            alias: alias.to_string(),
            addr: addr.map(|s| s.to_string()),
            has_key,
            tier: tier.to_string(),
        }
    }

    #[test]
    fn columns_derive_from_state() {
        assert!(card("a", "", Some("h:1"), true, "managed").board_col() == BoardCol::Ready);
        assert!(
            card("b", "", Some("h:1"), false, "stranger").board_col() == BoardCol::FirstContact
        );
        assert!(card("c", "", None, false, "stranger").board_col() == BoardCol::NeedsAddress);
        assert!(card("d", "", Some("  "), true, "managed").board_col() == BoardCol::NeedsAddress);
    }

    #[test]
    fn filter_matches_id_alias_tier_addr() {
        let c = card(
            "goliath",
            "Goliath",
            Some("127.0.0.1:21120"),
            true,
            "trusted_helper",
        );
        assert!(c.matches(""));
        assert!(c.matches("gol"));
        assert!(c.matches("GOLIATH"));
        assert!(c.matches("127.0.0"));
        assert!(c.matches("trusted"));
        assert!(!c.matches("zebra"));
    }

    #[test]
    fn visible_sorted_and_counted() {
        let b = BoardState::new(vec![
            card("zulu", "", Some("h:1"), true, "managed"),
            card("alpha", "", Some("h:2"), true, "managed"),
            card("new", "", Some("h:3"), false, "stranger"),
        ]);
        let ready = b.visible_in(BoardCol::Ready);
        assert!(ready.len() == 2);
        assert!(ready[0].id == "alpha");
        assert!(ready[1].id == "zulu");
        assert!(b.count_in(BoardCol::FirstContact) == 1);
        assert!(b.count_in(BoardCol::NeedsAddress) == 0);
        assert!(b.total_shown() == 3);
    }

    #[test]
    fn display_name_falls_back_to_id() {
        let c = card("peer-1", "", None, false, "stranger");
        assert!(c.display_name() == "peer-1");
        let d = card("peer-2", "Desk", None, false, "stranger");
        assert!(d.display_name() == "Desk");
    }
}
