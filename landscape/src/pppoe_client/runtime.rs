use landscape_common::service::{ServiceStatus, WatchService};
use tokio::sync::oneshot;

use super::session::PPPoEClientManager;
use super::state::TagValue;
use super::PPPoEClientConfig;
use crate::pppoe_client::DEFAULT_TIME_OUT;
use crate::route::IpRouteService;
use landscape_ebpf::pppoe;

pub async fn create_pppoe_client(
    config: PPPoEClientConfig,
    service_status: WatchService,
    route_service: Option<IpRouteService>,
) {
    service_status.just_change_status(ServiceStatus::Staring);
    tracing::info!(
        "starting native PPPoE client on iface={} ifindex={} requested_mru={} default_router={}",
        config.iface_name,
        config.index,
        config.requested_mru,
        config.default_router
    );

    let Ok((tx, mut rx)) = pppoe::start(config.index).await else {
        tracing::error!(
            "failed to start PPPoE raw socket on iface={} ifindex={}",
            config.iface_name,
            config.index
        );
        service_status.just_change_status(ServiceStatus::Failed);
        return;
    };

    let mut pkt_manager = PPPoEClientManager::new(
        config.iface_mac,
        config.requested_mru,
        config.peer_id.clone(),
        config.password.clone(),
    );

    let mut bpf_thread_notice = None;
    let mut exited_with_error = false;

    let mut timeout_times = 0_u64;
    let resend_timeout_timer = tokio::time::sleep(tokio::time::Duration::from_secs(0));
    tokio::pin!(resend_timeout_timer);

    let echo_timeout_timer = tokio::time::sleep(tokio::time::Duration::from_secs(3));
    tokio::pin!(echo_timeout_timer);
    resend_timeout_timer
        .as_mut()
        .reset(tokio::time::Instant::now() + tokio::time::Duration::from_secs(DEFAULT_TIME_OUT));

    let mut service_status_rx = service_status.subscribe();
    loop {
        tokio::select! {
            receive_data = rx.recv() => {
                if let Some(receive_data) = receive_data {
                    pkt_manager.handle_packet(*receive_data, &tx).await;
                    if pkt_manager.error_count > 10 {
                        tracing::error!(
                            "native PPPoE hit fatal negotiation threshold on iface={} error_count={}",
                            config.iface_name,
                            pkt_manager.error_count
                        );
                        exited_with_error = true;
                        break;
                    }

                    if bpf_thread_notice.is_none() && pkt_manager.can_enable_ebpf_prog() {
                        bpf_thread_notice = pkt_manager.enable_ebpf(&config, route_service.clone()).await;
                        if bpf_thread_notice.is_some() {
                            tracing::info!(
                                "native PPPoE session is fully established on iface={} and eBPF is enabled",
                                config.iface_name
                            );
                            service_status.just_change_status(ServiceStatus::Running);
                        } else {
                            tracing::error!(
                                "native PPPoE reached ready state but failed to apply system/eBPF setup on iface={}",
                                config.iface_name
                            );
                            exited_with_error = true;
                            break;
                        }
                    }
                    timeout_times = 0;
                    resend_timeout_timer.as_mut().reset(
                        tokio::time::Instant::now() + tokio::time::Duration::from_secs(DEFAULT_TIME_OUT),
                    );
                } else {
                    tracing::error!("PPPoE raw receive channel closed unexpectedly on iface={}", config.iface_name);
                    exited_with_error = true;
                    break;
                }
            },
            _ = &mut resend_timeout_timer => {
                if timeout_times > 3 {
                    tracing::error!(
                        "PPPoE negotiation timed out on iface={} after {} resend attempts",
                        config.iface_name,
                        timeout_times
                    );
                    exited_with_error = true;
                    break;
                }
                pkt_manager.send_packet(&tx).await;
                timeout_times += 1;
                resend_timeout_timer.as_mut().reset(
                    tokio::time::Instant::now()
                        + tokio::time::Duration::from_secs(timeout_times * 2 * DEFAULT_TIME_OUT),
                );
            }
            _ = &mut echo_timeout_timer => {
                if let Some((cueernt_timeout_times, wait_time)) = pkt_manager.get_keep_alive_pkt(&tx).await {
                    if cueernt_timeout_times > 5 {
                        tracing::error!(
                            "PPPoE LCP echo keepalive timed out on iface={} echo_failures={}",
                            config.iface_name,
                            cueernt_timeout_times
                        );
                        exited_with_error = true;
                        break;
                    }
                    echo_timeout_timer
                        .as_mut()
                        .reset(tokio::time::Instant::now() + tokio::time::Duration::from_secs(wait_time));
                } else {
                    echo_timeout_timer
                        .as_mut()
                        .reset(tokio::time::Instant::now() + tokio::time::Duration::from_secs(3));
                }
            }
            change_result = service_status_rx.changed() => {
                if change_result.is_err() {
                    tracing::error!("PPPoE service watcher closed unexpectedly on iface={}", config.iface_name);
                    exited_with_error = true;
                    break;
                }

                if matches!(*service_status_rx.borrow(), ServiceStatus::Stopping) {
                    if !pkt_manager.lcp_status.termination.0 {
                        pkt_manager.lcp_status.termination = (true, TagValue::Nak(()));
                        pkt_manager.send_packet(&tx).await;
                    }

                    tracing::info!("stopping native PPPoE client on iface={}", config.iface_name);
                    break;
                }
            }
        }
    }

    if let Some(bpf_thread_notice) = bpf_thread_notice {
        let (tx, rx) = oneshot::channel::<()>();
        if let Ok(()) = bpf_thread_notice.send(tx) {
            let _ = rx.await;
        }
    }

    if matches!(*service_status.0.borrow(), ServiceStatus::Stopping) || !exited_with_error {
        service_status.just_change_status(ServiceStatus::Stop);
    } else {
        service_status.just_change_status(ServiceStatus::Failed);
    }
    tracing::info!(
        "native PPPoE client down on iface={} status={:?}",
        config.iface_name,
        *service_status.0.borrow()
    );
}
