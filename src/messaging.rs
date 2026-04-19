use crate::{
    CONTROL_ALPN, ControlSession, EstablishedPairing, IrohBootstrapBundle, MessagingPeerKind,
    ShortCode,
};
use anyhow::{Context, Result, bail};
use iroh::{Endpoint, endpoint::presets};
use iroh_tickets::endpoint::EndpointTicket;
use std::time::{Duration, SystemTime};

#[derive(Clone, Debug)]
pub struct MessagingProbeOutcome {
    pub code: ShortCode,
    pub left_kind: MessagingPeerKind,
    pub right_kind: MessagingPeerKind,
    pub left_sent: String,
    pub right_received: String,
    pub right_sent: String,
    pub left_received: String,
}

pub async fn run_local_message_probe(
    code: ShortCode,
    left_kind: MessagingPeerKind,
    right_kind: MessagingPeerKind,
    left_text: impl Into<String>,
    right_text: impl Into<String>,
) -> Result<MessagingProbeOutcome> {
    let left_text = left_text.into();
    let right_text = right_text.into();
    let now = SystemTime::now();
    let ttl = Duration::from_secs(60);
    let expires_at = unix_secs(now + ttl);

    let left = new_endpoint().await.context("bind left message endpoint")?;
    let right = new_endpoint()
        .await
        .context("bind right message endpoint")?;

    let left_bundle = IrohBootstrapBundle::new(
        EndpointTicket::new(left.addr()),
        left_kind.capabilities(),
        Some(left_kind.label().to_string()),
        expires_at,
    );
    let right_bundle = IrohBootstrapBundle::new(
        EndpointTicket::new(right.addr()),
        right_kind.capabilities(),
        Some(right_kind.label().to_string()),
        expires_at,
    );

    let (left_pairing, right_pairing) = exchange_pairing(code.clone(), now, ttl)?;
    let left_remote_bundle = right_pairing
        .open_bootstrap(
            &left_pairing
                .seal_bootstrap(&left_bundle)
                .context("seal left bootstrap")?,
        )
        .context("open left bootstrap on right peer")?;
    let right_remote_bundle = left_pairing
        .open_bootstrap(
            &right_pairing
                .seal_bootstrap(&right_bundle)
                .context("seal right bootstrap")?,
        )
        .context("open right bootstrap on left peer")?;

    let right_text_for_task = right_text.clone();
    let right_bundle_for_task = right_bundle.clone();
    let left_remote_bundle_for_task = left_remote_bundle.clone();
    let right_for_task = right.clone();
    let accept_task = tokio::spawn(async move {
        let incoming = right_for_task.accept().await.ok_or_else(|| {
            anyhow::anyhow!("right endpoint closed before accepting control connection")
        })?;
        let connection = incoming
            .accept()
            .context("accept right-side control connection")?
            .await
            .context("complete right-side control handshake")?;
        let mut session = ControlSession::accept(
            connection,
            &right_pairing,
            &right_bundle_for_task,
            &left_remote_bundle_for_task,
        )
        .await
        .context("accept control session")?;
        let incoming = session
            .receive_message()
            .await
            .context("receive left-side message")?
            .ok_or_else(|| anyhow::anyhow!("left peer disconnected before sending a message"))?;
        session
            .send_message(right_text_for_task.clone())
            .await
            .context("send right-side message")?;
        session
            .finish_sending()
            .context("finish right-side control stream")?;
        let remote_closed = session
            .receive_message()
            .await
            .context("wait for left-side stream shutdown")?;
        if remote_closed.is_some() {
            bail!("left peer sent extra message after finishing expected exchange");
        }
        session
            .wait_for_send_completion()
            .await
            .context("wait for right-side send completion")?;
        Ok::<String, anyhow::Error>(incoming.body)
    });

    let mut left_session =
        ControlSession::connect(&left, &left_pairing, &left_bundle, &right_remote_bundle)
            .await
            .context("connect control session")?;
    left_session
        .send_message(left_text.clone())
        .await
        .context("send left-side message")?;
    left_session
        .finish_sending()
        .context("finish left-side control stream")?;
    let received_on_left = left_session
        .receive_message()
        .await
        .context("receive right-side message")?
        .ok_or_else(|| anyhow::anyhow!("right peer disconnected before sending a message"))?;
    let remote_closed = left_session
        .receive_message()
        .await
        .context("wait for right-side stream shutdown")?;
    if remote_closed.is_some() {
        bail!("right peer sent extra message after finishing expected exchange");
    }
    left_session
        .wait_for_send_completion()
        .await
        .context("wait for left-side send completion")?;

    let received_on_right = accept_task
        .await
        .context("join right-side message task")??;

    left.close().await;
    right.close().await;

    if received_on_right != left_text {
        bail!("right peer received unexpected message body");
    }
    if received_on_left.body != right_text {
        bail!("left peer received unexpected message body");
    }

    Ok(MessagingProbeOutcome {
        code,
        left_kind,
        right_kind,
        left_sent: left_text,
        right_received: received_on_right,
        right_sent: right_text,
        left_received: received_on_left.body,
    })
}

async fn new_endpoint() -> Result<Endpoint> {
    Endpoint::builder(presets::N0)
        .alpns(vec![CONTROL_ALPN.to_vec()])
        .bind()
        .await
        .context("bind control endpoint")
}

fn exchange_pairing(
    code: ShortCode,
    now: SystemTime,
    ttl: Duration,
) -> Result<(EstablishedPairing, EstablishedPairing)> {
    let mut left = crate::PairingHandshake::new(code.clone(), now, ttl);
    let mut right = crate::PairingHandshake::new(code, now, ttl);

    let left_pake = left.outbound_pake_message().to_vec();
    let right_pake = right.outbound_pake_message().to_vec();

    let left_pairing = left.finish(&right_pake, now)?.clone();
    let right_pairing = right.finish(&left_pake, now)?.clone();
    Ok((left_pairing, right_pairing))
}

fn unix_secs(value: SystemTime) -> u64 {
    value
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("time is after unix epoch")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::run_local_message_probe;
    use crate::{
        CONTROL_ALPN, ControlSession, IrohBootstrapBundle, MessagingPeerKind, PairingHandshake,
        ShortCode,
    };
    use anyhow::{Context, Result};
    use iroh::{Endpoint, endpoint::presets};
    use iroh_tickets::endpoint::EndpointTicket;
    use std::{str::FromStr, time::Duration};

    #[tokio::test]
    async fn local_message_probe_exchanges_messages() {
        let code = ShortCode::from_str("2048-badar-celen-votun").unwrap();
        let outcome = run_local_message_probe(
            code.clone(),
            MessagingPeerKind::Cli,
            MessagingPeerKind::Web,
            "hello from cli",
            "hello from web",
        )
        .await
        .unwrap();

        assert_eq!(outcome.code, code);
        assert_eq!(outcome.right_received, "hello from cli");
        assert_eq!(outcome.left_received, "hello from web");
    }

    #[tokio::test]
    async fn duplicate_messages_are_ignored() {
        let setup = setup_pair(MessagingPeerKind::Cli, MessagingPeerKind::Web)
            .await
            .unwrap();
        let right = setup.right_endpoint.clone();
        let right_pairing = setup.right_pairing.clone();
        let right_bundle = setup.right_bundle.clone();
        let left_remote_bundle = setup.left_bundle.clone();

        let accept_task = tokio::spawn(async move {
            let incoming = right.accept().await.ok_or_else(|| {
                anyhow::anyhow!("right endpoint closed before accepting control connection")
            })?;
            let connection = incoming
                .accept()
                .context("accept incoming control connection")?
                .await
                .context("complete incoming control connection")?;
            let mut session = ControlSession::accept(
                connection,
                &right_pairing,
                &right_bundle,
                &left_remote_bundle,
            )
            .await
            .context("accept control session")?;
            let first = session
                .receive_message()
                .await
                .context("receive first message")?
                .ok_or_else(|| anyhow::anyhow!("missing first message"))?;
            let second = session
                .receive_message()
                .await
                .context("receive second message")?
                .ok_or_else(|| anyhow::anyhow!("missing second message"))?;
            let remote_closed = session
                .receive_message()
                .await
                .context("wait for left stream shutdown")?;
            assert!(remote_closed.is_none());
            session
                .finish_sending()
                .context("finish right-side control stream")?;
            session
                .wait_for_send_completion()
                .await
                .context("wait for right-side send completion")?;
            Ok::<(String, String), anyhow::Error>((first.body, second.body))
        });

        let mut left_session = ControlSession::connect(
            &setup.left_endpoint,
            &setup.left_pairing,
            &setup.left_bundle,
            &setup.right_bundle,
        )
        .await
        .unwrap();
        left_session
            .send_message_with_id(42, "duplicate".to_string())
            .await
            .unwrap();
        left_session
            .send_message_with_id(42, "duplicate".to_string())
            .await
            .unwrap();
        left_session
            .send_message_with_id(43, "next".to_string())
            .await
            .unwrap();
        left_session.finish_sending().unwrap();
        left_session.wait_for_send_completion().await.unwrap();

        let (first, second) = accept_task.await.unwrap().unwrap();
        assert_eq!(first, "duplicate");
        assert_eq!(second, "next");
    }

    #[tokio::test]
    async fn disconnect_is_detected_after_binding() {
        let setup = setup_pair(MessagingPeerKind::Web, MessagingPeerKind::Web)
            .await
            .unwrap();
        let right = setup.right_endpoint.clone();
        let right_pairing = setup.right_pairing.clone();
        let right_bundle = setup.right_bundle.clone();
        let left_remote_bundle = setup.left_bundle.clone();

        let accept_task = tokio::spawn(async move {
            let incoming = right.accept().await.ok_or_else(|| {
                anyhow::anyhow!("right endpoint closed before accepting control connection")
            })?;
            let connection = incoming
                .accept()
                .context("accept incoming control connection")?
                .await
                .context("complete incoming control connection")?;
            let mut session = ControlSession::accept(
                connection,
                &right_pairing,
                &right_bundle,
                &left_remote_bundle,
            )
            .await
            .context("accept control session")?;
            let message = session
                .receive_message()
                .await
                .context("receive after disconnect")?;
            Ok::<Option<String>, anyhow::Error>(message.map(|item| item.body))
        });

        let left_session = ControlSession::connect(
            &setup.left_endpoint,
            &setup.left_pairing,
            &setup.left_bundle,
            &setup.right_bundle,
        )
        .await
        .unwrap();
        let mut left_session = left_session;
        left_session.finish_sending().unwrap();
        left_session.wait_for_send_completion().await.unwrap();

        let received = accept_task.await.unwrap().unwrap();
        assert!(received.is_none());
    }

    struct TestPairSetup {
        left_endpoint: Endpoint,
        right_endpoint: Endpoint,
        left_pairing: crate::EstablishedPairing,
        right_pairing: crate::EstablishedPairing,
        left_bundle: IrohBootstrapBundle,
        right_bundle: IrohBootstrapBundle,
    }

    async fn setup_pair(
        left_kind: MessagingPeerKind,
        right_kind: MessagingPeerKind,
    ) -> Result<TestPairSetup> {
        let now = std::time::SystemTime::now();
        let ttl = Duration::from_secs(60);
        let expires_at = now
            .checked_add(ttl)
            .expect("duration is valid")
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .expect("time is after unix epoch")
            .as_secs();
        let code = ShortCode::from_str("2048-badar-celen-votun").unwrap();

        let left_endpoint = Endpoint::builder(presets::N0)
            .alpns(vec![CONTROL_ALPN.to_vec()])
            .bind()
            .await
            .context("bind left endpoint")?;
        let right_endpoint = Endpoint::builder(presets::N0)
            .alpns(vec![CONTROL_ALPN.to_vec()])
            .bind()
            .await
            .context("bind right endpoint")?;

        let left_bundle = IrohBootstrapBundle::new(
            EndpointTicket::new(left_endpoint.addr()),
            left_kind.capabilities(),
            Some(left_kind.label().to_string()),
            expires_at,
        );
        let right_bundle = IrohBootstrapBundle::new(
            EndpointTicket::new(right_endpoint.addr()),
            right_kind.capabilities(),
            Some(right_kind.label().to_string()),
            expires_at,
        );

        let mut left_handshake = PairingHandshake::new(code.clone(), now, ttl);
        let mut right_handshake = PairingHandshake::new(code, now, ttl);
        let left_pake = left_handshake.outbound_pake_message().to_vec();
        let right_pake = right_handshake.outbound_pake_message().to_vec();
        let left_pairing = left_handshake.finish(&right_pake, now)?.clone();
        let right_pairing = right_handshake.finish(&left_pake, now)?.clone();

        let left_remote_bundle =
            right_pairing.open_bootstrap(&left_pairing.seal_bootstrap(&left_bundle)?)?;
        let right_remote_bundle =
            left_pairing.open_bootstrap(&right_pairing.seal_bootstrap(&right_bundle)?)?;
        assert_eq!(left_remote_bundle, left_bundle);
        assert_eq!(right_remote_bundle, right_bundle);

        Ok(TestPairSetup {
            left_endpoint,
            right_endpoint,
            left_pairing,
            right_pairing,
            left_bundle,
            right_bundle,
        })
    }
}
