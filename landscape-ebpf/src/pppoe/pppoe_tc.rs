use std::{mem::MaybeUninit, os::fd::AsFd};

use libbpf_rs::{
    query::ProgInfoIter,
    skel::{OpenSkel, SkelBuilder},
    ErrorKind, Xdp, XdpFlags, TC_EGRESS,
};
use tokio::sync::oneshot::error::TryRecvError;

use crate::{
    bpf_error::{LandscapeEbpfError, LdEbpfResult},
    landscape::TcHookProxy,
    map_setting::reuse_pinned_map_or_recreate,
    pipeline::wan_tc::{self, WanTcPipelineHandle},
    PPPOE_EGRESS_PRIORITY,
};

mod landscape_pppoe {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bpf_rs/pppoe.skel.rs"));
}

pub async fn create_pppoe_tc_ebpf_3(
    ifindex: u32,
    session_id: u16,
    _mtu: u16,
) -> LdEbpfResult<tokio::sync::oneshot::Sender<tokio::sync::oneshot::Sender<()>>> {
    let (notice_tx, mut notice_rx) =
        tokio::sync::oneshot::channel::<tokio::sync::oneshot::Sender<()>>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);

    std::thread::spawn(move || {
        let pipeline = match WanTcPipelineHandle::acquire(ifindex) {
            Ok(pipeline) => pipeline,
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                return;
            }
        };

        let builder = landscape_pppoe::PppoeSkelBuilder::default();
        let mut open_object = MaybeUninit::uninit();
        let mut pppoe_open = match crate::bpf_ctx!(
            builder.open(&mut open_object),
            "pppoe tc open skeleton failed"
        ) {
            Ok(pppoe_open) => pppoe_open,
            Err(e) => {
                let _ = ready_tx.send(Err(e.into()));
                return;
            }
        };
        let ingress_path = wan_tc::wan_tc_pipeline_ingress_path(ifindex);
        let egress_path = wan_tc::wan_tc_pipeline_egress_path(ifindex);
        reuse_pinned_map_or_recreate(&mut pppoe_open.maps.ingress_stage_progs, &ingress_path);
        reuse_pinned_map_or_recreate(&mut pppoe_open.maps.egress_stage_progs, &egress_path);

        let rodata_data =
            pppoe_open.maps.rodata_data.as_deref_mut().expect("rodata is not memory mapped");
        rodata_data.session_id = session_id;

        let pppoe_skel = match crate::bpf_ctx!(pppoe_open.load(), "pppoe tc load skeleton failed") {
            Ok(pppoe_skel) => pppoe_skel,
            Err(e) => {
                let _ = ready_tx.send(Err(e.into()));
                return;
            }
        };

        if let Err(e) =
            pipeline.register_pppoe(&pppoe_skel.progs.pppoe_ingress, &pppoe_skel.progs.pppoe_egress)
        {
            let _ = ready_tx.send(Err(e));
            return;
        }

        let xdp_ingress = Xdp::new(pppoe_skel.progs.pppoe_xdp_ingress.as_fd());
        if let Err(e) = attach_pppoe_xdp(&xdp_ingress, ifindex as i32) {
            pipeline.unregister_pppoe();
            let _ = ready_tx.send(Err(e));
            return;
        }

        tracing::info!(
            "pppoe xdp ingress and tc pipeline registered for ifindex={} session_id={}",
            ifindex,
            session_id
        );
        let _ = ready_tx.send(Ok(()));

        let call_back = loop {
            match notice_rx.try_recv() {
                Ok(call_back) => break Some(call_back),
                Err(TryRecvError::Empty) => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(TryRecvError::Closed) => break None,
            }
        };

        pipeline.unregister_pppoe();
        if let Err(e) =
            xdp_ingress.detach(ifindex as i32, XdpFlags::SKB_MODE | XdpFlags::UPDATE_IF_NOEXIST)
        {
            tracing::warn!("pppoe xdp ingress detach failed for ifindex={}: {}", ifindex, e);
        }
        tracing::info!("pppoe tc pipeline unregistered for ifindex={}", ifindex);

        if let Some(call_back) = call_back {
            let _ = call_back.send(());
        }
        drop(pppoe_skel);
    });

    match ready_rx.recv() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(e),
        Err(e) => {
            tracing::error!("pppoe tc ready channel closed for ifindex={}: {}", ifindex, e);
            return Err(LandscapeEbpfError::Internal(format!(
                "pppoe tc ready channel closed for ifindex={ifindex}: {e}"
            )));
        }
    }

    Ok(notice_tx)
}

pub async fn create_pppoe_tc_ebpf<'a>(
    ifindex: u32,
    session_id: u16,
    obj: &'a mut MaybeUninit<libbpf_rs::OpenObject>,
) -> (tokio::sync::broadcast::Sender<()>, landscape_pppoe::PppoeSkel<'a>) {
    let pppoe_builder = landscape_pppoe::PppoeSkelBuilder::default();

    let mut pppoe_open: landscape_pppoe::OpenPppoeSkel<'a> =
        crate::bpf_ctx!(pppoe_builder.open(obj), "pppoe_tc open skeleton failed").unwrap();
    let rodata_data =
        pppoe_open.maps.rodata_data.as_deref_mut().expect("rodata is not memory mapped");

    rodata_data.session_id = session_id;
    let pppoe_skel: landscape_pppoe::PppoeSkel<'a> =
        crate::bpf_ctx!(pppoe_open.load(), "pppoe_tc load skeleton failed").unwrap();

    let mut pppoe_egress_builder = TcHookProxy::new(
        &pppoe_skel.progs.pppoe_egress,
        ifindex as i32,
        TC_EGRESS,
        PPPOE_EGRESS_PRIORITY,
    );

    pppoe_egress_builder.attach();

    let (notice_tx, mut notice_rx) = tokio::sync::broadcast::channel::<()>(1);

    std::thread::spawn(move || {
        let _ = notice_rx.blocking_recv();
        drop(pppoe_egress_builder);
    });
    (notice_tx, pppoe_skel)
}

fn attach_pppoe_xdp(xdp_ingress: &Xdp<'_>, ifindex: i32) -> crate::bpf_error::LdEbpfResult<()> {
    match xdp_ingress.attach(ifindex, XdpFlags::SKB_MODE | XdpFlags::UPDATE_IF_NOEXIST) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => {
            tracing::warn!(
                "pppoe xdp already attached on ifindex={}, checking whether it is our own program",
                ifindex
            );
            let existing_id =
                xdp_ingress.query_id(ifindex, XdpFlags::SKB_MODE).map_err(|source| {
                    crate::bpf_error::LandscapeEbpfError::Context {
                        context: format!("pppoe xdp query failed on ifindex {ifindex}"),
                        source,
                    }
                })?;

            let mut prog_iter = ProgInfoIter::default();
            let Some(existing_prog) = prog_iter.find(|prog| prog.id == existing_id) else {
                return Err(crate::bpf_error::LandscapeEbpfError::Internal(format!(
                    "pppoe xdp existing program id {existing_id} not found on ifindex {ifindex}"
                )));
            };

            let existing_name = existing_prog.name.as_c_str().to_string_lossy();
            if !existing_name.contains("pppoe_xdp") {
                return Err(crate::bpf_error::LandscapeEbpfError::Internal(format!(
                    "pppoe xdp already attached on ifindex {ifindex} by non-pppoe program {existing_name}"
                )));
            }

            tracing::warn!(
                "detaching stale pppoe xdp program id={} name={} on ifindex={}",
                existing_id,
                existing_name,
                ifindex
            );
            xdp_ingress.detach(ifindex, XdpFlags::SKB_MODE).map_err(|source| {
                crate::bpf_error::LandscapeEbpfError::Context {
                    context: format!("pppoe xdp detach stale program failed on ifindex {ifindex}"),
                    source,
                }
            })?;
            xdp_ingress.attach(ifindex, XdpFlags::SKB_MODE | XdpFlags::UPDATE_IF_NOEXIST).map_err(
                |source| crate::bpf_error::LandscapeEbpfError::Context {
                    context: format!(
                        "pppoe xdp attach after stale detach failed on ifindex {ifindex}"
                    ),
                    source,
                },
            )?;
            Ok(())
        }
        Err(err) => Err(err.into()),
    }
}
