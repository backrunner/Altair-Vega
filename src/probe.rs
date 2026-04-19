use crate::{
    IrohBootstrapBundle, PairingHandshake, PeerCapabilities, ShortCode, pairing::EstablishedPairing,
};
use anyhow::{Context, Result, bail};
use iroh::{Endpoint, endpoint::presets};
use iroh_tickets::endpoint::EndpointTicket;
use std::time::{Duration, SystemTime};

const PROBE_ALPN: &[u8] = b"altair-vega/pairing-probe/1";

#[derive(Clone, Debug)]
pub struct PairingProbeOutcome {
    pub code: ShortCode,
    pub left_ticket: EndpointTicket,
    pub right_ticket: EndpointTicket,
}

pub async fn run_local_pairing_probe(code: ShortCode) -> Result<PairingProbeOutcome> {
    let now = SystemTime::now();
    let ttl = Duration::from_secs(60);
    let expires_at = unix_secs(now + ttl);

    let left = new_endpoint().await.context("bind left endpoint")?;
    let right = new_endpoint().await.context("bind right endpoint")?;

    let left_ticket = EndpointTicket::new(left.addr());
    let right_ticket = EndpointTicket::new(right.addr());

    let left_bundle = IrohBootstrapBundle::new(
        left_ticket.clone(),
        PeerCapabilities::cli(),
        Some("left-demo".to_string()),
        expires_at,
    );
    let right_bundle = IrohBootstrapBundle::new(
        right_ticket.clone(),
        PeerCapabilities::cli(),
        Some("right-demo".to_string()),
        expires_at,
    );

    let (left_pairing, right_pairing) = exchange_pairing(code.clone(), now, ttl)?;
    let left_envelope = left_pairing
        .seal_bootstrap(&left_bundle)
        .context("seal left bootstrap")?;
    let right_envelope = right_pairing
        .seal_bootstrap(&right_bundle)
        .context("seal right bootstrap")?;

    let decrypted_left = right_pairing
        .open_bootstrap(&left_envelope)
        .context("open left bootstrap")?;
    let decrypted_right = left_pairing
        .open_bootstrap(&right_envelope)
        .context("open right bootstrap")?;

    if left_pairing.connection_binding_tag(&decrypted_right)
        != right_pairing.connection_binding_tag(&right_bundle)
    {
        bail!("left pairing binding tag mismatch");
    }
    if right_pairing.connection_binding_tag(&decrypted_left)
        != left_pairing.connection_binding_tag(&left_bundle)
    {
        bail!("right pairing binding tag mismatch");
    }

    probe_connection(&right, &left, decrypted_left.endpoint_ticket.clone())
        .await
        .context("probe right->left bootstrap")?;
    probe_connection(&left, &right, decrypted_right.endpoint_ticket.clone())
        .await
        .context("probe left->right bootstrap")?;

    left.close().await;
    right.close().await;

    Ok(PairingProbeOutcome {
        code,
        left_ticket,
        right_ticket,
    })
}

fn exchange_pairing(
    code: ShortCode,
    now: SystemTime,
    ttl: Duration,
) -> Result<(EstablishedPairing, EstablishedPairing)> {
    let mut left = PairingHandshake::new(code.clone(), now, ttl);
    let mut right = PairingHandshake::new(code, now, ttl);

    let left_pake = left.outbound_pake_message().to_vec();
    let right_pake = right.outbound_pake_message().to_vec();

    let left_pairing = left.finish(&right_pake, now)?.clone();
    let right_pairing = right.finish(&left_pake, now)?.clone();

    Ok((left_pairing, right_pairing))
}

async fn new_endpoint() -> Result<Endpoint> {
    Endpoint::builder(presets::N0)
        .alpns(vec![PROBE_ALPN.to_vec()])
        .bind()
        .await
        .context("bind endpoint")
}

async fn probe_connection(
    dialer: &Endpoint,
    listener: &Endpoint,
    ticket: EndpointTicket,
) -> Result<()> {
    let listener = listener.clone();
    let accept = tokio::spawn(async move {
        let incoming = listener
            .accept()
            .await
            .ok_or_else(|| anyhow::anyhow!("listener closed before accepting probe"))?;
        let connection = incoming
            .accept()
            .context("accept incoming probe")?
            .await
            .context("complete incoming probe")?;
        connection.close(0u32.into(), b"probe-ok");
        Ok::<(), anyhow::Error>(())
    });

    let connection = dialer
        .connect(ticket.endpoint_addr().clone(), PROBE_ALPN)
        .await
        .context("dial probe connection")?;
    connection.close(0u32.into(), b"probe-ok");
    accept.await.context("join accept task")??;
    Ok(())
}

fn unix_secs(value: SystemTime) -> u64 {
    value
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("time is after unix epoch")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::run_local_pairing_probe;
    use crate::ShortCode;
    use std::str::FromStr;

    #[tokio::test]
    async fn local_probe_bootstraps_real_iroh_tickets() {
        let code = ShortCode::from_str("2048-badar-celen-votun").unwrap();
        let outcome = run_local_pairing_probe(code.clone()).await.unwrap();
        assert_eq!(outcome.code, code);
    }
}
