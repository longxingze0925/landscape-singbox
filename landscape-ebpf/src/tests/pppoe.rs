use std::mem::MaybeUninit;

use etherparse::PacketBuilder;
use libbpf_rs::{
    skel::{OpenSkel, SkelBuilder as _},
    ProgramInput,
};

mod pppoe_skel {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bpf_rs/pppoe.skel.rs"));
}

const SESSION_ID: u16 = 0x2233;

fn build_ipv4_packet() -> Vec<u8> {
    let builder = PacketBuilder::ethernet2(
        [0x02, 0x11, 0x22, 0x33, 0x44, 0x55],
        [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
    )
    .ipv4([10, 0, 0, 1], [10, 0, 0, 2], 64)
    .tcp(12345, 443, 0x1000_0000, 8192)
    .syn();
    let payload = [0x11, 0x22, 0x33];
    let mut packet = Vec::with_capacity(builder.size(payload.len()));
    builder.write(&mut packet, &payload).unwrap();
    packet
}

fn build_ipv6_packet() -> Vec<u8> {
    let builder = PacketBuilder::ethernet2(
        [0x02, 0x11, 0x22, 0x33, 0x44, 0x55],
        [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
    )
    .ipv6(
        [0xfd, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
        [0xfd, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2],
        64,
    )
    .tcp(12345, 443, 0x1000_0000, 8192)
    .syn();
    let payload = [0x11, 0x22, 0x33];
    let mut packet = Vec::with_capacity(builder.size(payload.len()));
    builder.write(&mut packet, &payload).unwrap();
    packet
}

#[allow(dead_code)]
fn pppoe_encap_header(session_id: u16, payload_len: u16, is_ipv6: bool) -> [u8; 22] {
    let mut buf = [0u8; 22];
    buf[0..6].copy_from_slice(&[0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
    buf[6..12].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    buf[12] = 0x88;
    buf[13] = 0x64;
    buf[14] = 0x11;
    buf[15] = 0x00;
    buf[16] = (session_id >> 8) as u8;
    buf[17] = session_id as u8;
    let ppp_len = payload_len + 2;
    buf[18] = (ppp_len >> 8) as u8;
    buf[19] = ppp_len as u8;
    if is_ipv6 {
        buf[20] = 0x00;
        buf[21] = 0x57;
    } else {
        buf[20] = 0x00;
        buf[21] = 0x21;
    }
    buf
}

#[allow(dead_code)]
fn build_pppoe_ipv4_packet() -> Vec<u8> {
    let ip_pkt = build_ipv4_packet();
    let ip_payload = &ip_pkt[14..];
    let pppoe_header = pppoe_encap_header(SESSION_ID, ip_payload.len() as u16, false);
    let mut full_pkt = Vec::with_capacity(22 + ip_payload.len());
    full_pkt.extend_from_slice(&pppoe_header);
    full_pkt.extend_from_slice(ip_payload);
    full_pkt
}

#[allow(dead_code)]
fn build_pppoe_ipv6_packet() -> Vec<u8> {
    let ip_pkt = build_ipv6_packet();
    let ip_payload = &ip_pkt[14..];
    let pppoe_header = pppoe_encap_header(SESSION_ID, ip_payload.len() as u16, true);
    let mut full_pkt = Vec::with_capacity(22 + ip_payload.len());
    full_pkt.extend_from_slice(&pppoe_header);
    full_pkt.extend_from_slice(ip_payload);
    full_pkt
}

#[test]
fn pppoe_egress_ipv4_adds_pppoe_header() {
    let builder = pppoe_skel::PppoeSkelBuilder::default();
    let mut open_object = MaybeUninit::uninit();
    let mut pppoe_open = builder.open(&mut open_object).unwrap();
    pppoe_open.maps.rodata_data.as_deref_mut().unwrap().session_id = SESSION_ID;
    let skel = pppoe_open.load().unwrap();

    let mut plain_pkt = build_ipv4_packet();
    let mut output = vec![0u8; plain_pkt.len() + 8];

    skel.progs
        .pppoe_egress
        .test_run(ProgramInput {
            data_in: Some(&mut plain_pkt),
            data_out: Some(&mut output),
            ..Default::default()
        })
        .expect("test_run pppoe_egress ipv4");

    assert_eq!(output[12], 0x88);
    assert_eq!(output[13], 0x64);
    assert_eq!(output[14], 0x11);
    assert_eq!(output[15], 0x00);
    assert_eq!(output[16], (SESSION_ID >> 8) as u8);
    assert_eq!(output[17], SESSION_ID as u8);
    assert_eq!(output[20], 0x00);
    assert_eq!(output[21], 0x21);
    assert_eq!(&output[22..plain_pkt.len() + 8], &plain_pkt[14..]);
}

#[test]
fn pppoe_egress_ipv6_adds_pppoe_header() {
    let builder = pppoe_skel::PppoeSkelBuilder::default();
    let mut open_object = MaybeUninit::uninit();
    let mut pppoe_open = builder.open(&mut open_object).unwrap();
    pppoe_open.maps.rodata_data.as_deref_mut().unwrap().session_id = SESSION_ID;
    let skel = pppoe_open.load().unwrap();

    let mut plain_pkt = build_ipv6_packet();
    let mut output = vec![0u8; plain_pkt.len() + 8];

    skel.progs
        .pppoe_egress
        .test_run(ProgramInput {
            data_in: Some(&mut plain_pkt),
            data_out: Some(&mut output),
            ..Default::default()
        })
        .expect("test_run pppoe_egress ipv6");

    assert_eq!(output[12], 0x88);
    assert_eq!(output[13], 0x64);
    assert_eq!(output[14], 0x11);
    assert_eq!(output[15], 0x00);
    assert_eq!(output[16], (SESSION_ID >> 8) as u8);
    assert_eq!(output[17], SESSION_ID as u8);
    assert_eq!(output[20], 0x00);
    assert_eq!(output[21], 0x57);
    assert_eq!(&output[22..plain_pkt.len() + 8], &plain_pkt[14..]);
}

#[test]
fn pppoe_egress_non_ip_passes_unchanged() {
    let builder = pppoe_skel::PppoeSkelBuilder::default();
    let mut open_object = MaybeUninit::uninit();
    let mut pppoe_open = builder.open(&mut open_object).unwrap();
    pppoe_open.maps.rodata_data.as_deref_mut().unwrap().session_id = SESSION_ID;
    let skel = pppoe_open.load().unwrap();

    let mut arp_pkt = [
        0xFFu8, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x02, 0x11, 0x22, 0x33, 0x44, 0x55, 0x08, 0x06, 0x00,
        0x01, 0x08, 0x00, 0x06, 0x04, 0x00, 0x01, 0x02, 0x11, 0x22, 0x33, 0x44, 0x55, 0xC0, 0xA8,
        0x01, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xC0, 0xA8, 0x01, 0x02,
    ]
    .to_vec();
    let original = arp_pkt.clone();
    let mut output = vec![0u8; arp_pkt.len() + 8];

    skel.progs
        .pppoe_egress
        .test_run(ProgramInput {
            data_in: Some(&mut arp_pkt),
            data_out: Some(&mut output),
            ..Default::default()
        })
        .expect("test_run pppoe_egress non-ip");

    assert_eq!(&output[..original.len()], &original[..]);
}

#[test]
fn pppoe_session_id_is_set_in_rodata() {
    let builder = pppoe_skel::PppoeSkelBuilder::default();
    let mut open_object = MaybeUninit::uninit();
    let mut pppoe_open = builder.open(&mut open_object).unwrap();
    let rodata = pppoe_open.maps.rodata_data.as_deref_mut().unwrap();
    rodata.session_id = SESSION_ID;
    assert_eq!(rodata.session_id, SESSION_ID);
    let skel = pppoe_open.load().unwrap();

    // Verify both programs loaded: simple test_run on empty data should work
    let mut empty = vec![0u8; 64];
    let mut out = vec![0u8; 64];
    skel.progs
        .pppoe_ingress
        .test_run(ProgramInput {
            data_in: Some(&mut empty),
            data_out: Some(&mut out),
            ..Default::default()
        })
        .expect("pppoe_ingress test_run");
    skel.progs
        .pppoe_egress
        .test_run(ProgramInput {
            data_in: Some(&mut empty),
            data_out: Some(&mut out),
            ..Default::default()
        })
        .expect("pppoe_egress test_run");
    skel.progs
        .pppoe_xdp_ingress
        .test_run(ProgramInput {
            data_in: Some(&empty),
            data_out: Some(&mut out),
            ..Default::default()
        })
        .expect("pppoe_xdp_ingress test_run");
}

#[test]
fn pppoe_ingress_non_ppp_passes_unchanged() {
    let builder = pppoe_skel::PppoeSkelBuilder::default();
    let mut open_object = MaybeUninit::uninit();
    let mut pppoe_open = builder.open(&mut open_object).unwrap();
    pppoe_open.maps.rodata_data.as_deref_mut().unwrap().session_id = SESSION_ID;
    let skel = pppoe_open.load().unwrap();

    let mut plain_pkt = build_ipv4_packet();
    let original = plain_pkt.clone();
    let mut output = vec![0u8; plain_pkt.len()];

    let ret = skel
        .progs
        .pppoe_ingress
        .test_run(ProgramInput {
            data_in: Some(&mut plain_pkt),
            data_out: Some(&mut output),
            ..Default::default()
        })
        .expect("test_run pppoe_ingress non-ppp");

    // Non-PPP packets should pass through (TC_ACT_UNSPEC = -1 = 0xFFFFFFFF)
    let expected_unspec = u32::MAX; // -1 as u32
    assert_eq!(ret.return_value, expected_unspec, "non-PPP packet should return TC_ACT_UNSPEC");
    assert_eq!(&output[..original.len()], &original[..]);
}

#[test]
fn pppoe_xdp_ingress_ipv4_removes_pppoe_header() {
    let builder = pppoe_skel::PppoeSkelBuilder::default();
    let mut open_object = MaybeUninit::uninit();
    let mut pppoe_open = builder.open(&mut open_object).unwrap();
    pppoe_open.maps.rodata_data.as_deref_mut().unwrap().session_id = SESSION_ID;
    let skel = pppoe_open.load().unwrap();

    let pppoe_pkt = build_pppoe_ipv4_packet();
    let plain_pkt = build_ipv4_packet();
    let mut output = vec![0u8; pppoe_pkt.len()];

    let ret = skel
        .progs
        .pppoe_xdp_ingress
        .test_run(ProgramInput {
            data_in: Some(&pppoe_pkt),
            data_out: Some(&mut output),
            ..Default::default()
        })
        .expect("test_run pppoe_xdp_ingress ipv4");

    assert_eq!(ret.return_value, 2, "XDP_PASS");
    let data = ret.data.expect("xdp output data");
    assert_eq!(data.len(), plain_pkt.len());
    assert_eq!(&data[..12], &pppoe_pkt[..12]);
    assert_eq!(data[12], 0x08);
    assert_eq!(data[13], 0x00);
    assert_eq!(&data[14..], &plain_pkt[14..]);
}

#[test]
fn pppoe_xdp_ingress_ipv6_removes_pppoe_header() {
    let builder = pppoe_skel::PppoeSkelBuilder::default();
    let mut open_object = MaybeUninit::uninit();
    let mut pppoe_open = builder.open(&mut open_object).unwrap();
    pppoe_open.maps.rodata_data.as_deref_mut().unwrap().session_id = SESSION_ID;
    let skel = pppoe_open.load().unwrap();

    let pppoe_pkt = build_pppoe_ipv6_packet();
    let plain_pkt = build_ipv6_packet();
    let mut output = vec![0u8; pppoe_pkt.len()];

    let ret = skel
        .progs
        .pppoe_xdp_ingress
        .test_run(ProgramInput {
            data_in: Some(&pppoe_pkt),
            data_out: Some(&mut output),
            ..Default::default()
        })
        .expect("test_run pppoe_xdp_ingress ipv6");

    assert_eq!(ret.return_value, 2, "XDP_PASS");
    let data = ret.data.expect("xdp output data");
    assert_eq!(data.len(), plain_pkt.len());
    assert_eq!(&data[..12], &pppoe_pkt[..12]);
    assert_eq!(data[12], 0x86);
    assert_eq!(data[13], 0xdd);
    assert_eq!(&data[14..], &plain_pkt[14..]);
}
