//! Link-layer state machine shared by Wi-Fi, DHCP and user actions.
//!
//! The manager still derives the externally visible device state from kernel
//! facts, but this machine records intent and makes every transition explicit.
//! In particular a CONNECTED signal never means "link up" until DHCP reports
//! BOUND, and a disconnect always passes through Disconnecting.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Down,
    Scanning,
    Authenticating,
    /// wpa_supplicant reported CTRL-EVENT-CONNECTED; DHCP is not done yet.
    Associated,
    ConfiguringIp,
    Connected,
    Disconnecting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkEvent {
    ScanRequested,
    ScanComplete,
    StartAuthentication,
    Associated,
    DhcpStart,
    DhcpBound,
    DhcpExpired,
    DisconnectRequested,
    Disconnected,
    LinkLost,
    Failure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTransition {
    pub state: LinkState,
    pub event: LinkEvent,
}

impl std::fmt::Display for InvalidTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid link transition {:?} + {:?}",
            self.state, self.event
        )
    }
}

impl std::error::Error for InvalidTransition {}

impl LinkState {
    /// Apply one event.  Invalid events are rejected instead of silently
    /// producing a plausible-looking state.
    pub fn transition(self, event: LinkEvent) -> Result<Self, InvalidTransition> {
        use LinkEvent as E;
        use LinkState as S;
        let next = match (self, event) {
            (S::Down | S::Connected, E::ScanRequested) => S::Scanning,
            (S::Scanning, E::ScanComplete) => S::Down,
            (S::Scanning | S::Down, E::StartAuthentication) => S::Authenticating,
            (S::Authenticating, E::Associated) => S::Associated,
            (S::Associated, E::DhcpStart) => S::ConfiguringIp,
            (S::ConfiguringIp, E::DhcpBound) => S::Connected,
            // Roaming: a fresh association while connected means DHCP should be
            // restarted, even if the address may end up unchanged.
            (S::Connected, E::Associated) => S::Associated,
            (S::Connected, E::DhcpStart) => S::ConfiguringIp,
            (S::Scanning | S::Authenticating | S::Associated | S::ConfiguringIp, E::Failure)
            | (
                S::Scanning | S::Authenticating | S::Associated | S::ConfiguringIp,
                E::DhcpExpired,
            )
            | (S::Scanning | S::Authenticating | S::Associated | S::ConfiguringIp, E::LinkLost) => {
                S::Down
            }
            (S::Down, E::Failure) => S::Down,
            (S::Connected, E::LinkLost) => S::Down,
            (_, E::DisconnectRequested) if self != S::Down => S::Disconnecting,
            (S::Disconnecting, E::Disconnected | E::LinkLost) => S::Down,
            (S::Down, E::Disconnected) => S::Down,
            _ => return Err(InvalidTransition { state: self, event }),
        };
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use LinkEvent as E;
    use LinkState as S;

    #[test]
    fn full_connection_path() {
        let mut state = S::Down;
        for event in [
            E::ScanRequested,
            E::StartAuthentication,
            E::Associated,
            E::DhcpStart,
            E::DhcpBound,
        ] {
            state = state.transition(event).expect("valid transition");
        }
        assert_eq!(state, S::Connected);
    }

    #[test]
    fn association_is_not_connected_until_dhcp() {
        let state = S::Authenticating.transition(E::Associated).unwrap();
        assert_eq!(state, S::Associated);
        assert_eq!(
            state.transition(E::DisconnectRequested).unwrap(),
            S::Disconnecting
        );
    }

    #[test]
    fn dhcp_failure_rolls_back() {
        let state = S::ConfiguringIp.transition(E::DhcpExpired).unwrap();
        assert_eq!(state, S::Down);
    }

    #[test]
    fn invalid_transition_is_rejected() {
        assert_eq!(
            S::Down.transition(E::DhcpBound),
            Err(InvalidTransition {
                state: S::Down,
                event: E::DhcpBound
            })
        );
    }
}
