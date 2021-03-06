// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::pdu::{ArpOp, ArpPdu};
use crate::{prelude::*, protocols::ethernet2, test};
use serde_yaml;
use std::{
    io::Cursor,
    time::{Duration, Instant},
};

#[test]
fn immediate_reply() {
    // tests to ensure that an are request results in a reply.
    let now = Instant::now();
    let mut alice = test::new_alice(now);
    let mut bob = test::new_bob(now);
    let mut carrie = test::new_carrie(now);

    // this test is written based on certain assumptions.
    let options = alice.options();
    assert_eq!(options.arp.request_timeout, Duration::from_secs(1));

    let fut = alice.arp_query(*test::carrie_ipv4_addr());
    let now = now + Duration::from_micros(1);
    assert!(fut.poll(now).is_none());

    alice.advance_clock(now);
    let request = {
        let event = alice.pop_event().unwrap();
        match &*event {
            Event::Transmit(datagram) => datagram.borrow().to_vec(),
            e => panic!("got unanticipated event `{:?}`", e),
        }
    };

    assert!(request.len() >= ethernet2::MIN_PAYLOAD_SIZE);

    // bob hasn't heard of alice before, so he will ignore the request.
    info!("passing ARP request to bob (should be ignored)...");
    match bob.receive(&request) {
        Err(Fail::Ignored { .. }) => (),
        x => panic!("expected Fail::Ignored {{}}, got `{:?}`", x),
    }
    let cache = bob.export_arp_cache();
    assert!(cache.get(test::alice_ipv4_addr()).is_none());

    carrie.receive(&request).unwrap();
    info!("passing ARP request to carrie...");
    let cache = carrie.export_arp_cache();
    assert_eq!(
        cache.get(test::alice_ipv4_addr()),
        Some(test::alice_link_addr())
    );

    carrie.advance_clock(now);
    let reply = {
        let event = carrie.pop_event().unwrap();
        match &*event {
            Event::Transmit(datagram) => datagram.borrow().to_vec(),
            e => panic!("got unanticipated event `{:?}`", e),
        }
    };

    info!("passing ARP reply back to alice...");
    alice.receive(&reply).unwrap();
    debug!(
        "ARP cache contains: \n{}",
        serde_yaml::to_string(&alice.export_arp_cache()).unwrap()
    );
    let now = now + Duration::from_micros(1);
    let link_addr = fut.poll(now).unwrap().unwrap();
    assert_eq!(*test::carrie_link_addr(), link_addr);
}

#[test]
fn slow_reply() {
    // tests to ensure that an are request results in a reply.
    let now = Instant::now();
    let mut alice = test::new_alice(now);
    let mut bob = test::new_bob(now);
    let mut carrie = test::new_carrie(now);

    // this test is written based on certain assumptions.
    let options = alice.options();
    assert!(options.arp.retry_count > 0);
    assert_eq!(options.arp.request_timeout, Duration::from_secs(1));

    let fut = alice.arp_query(*test::carrie_ipv4_addr());
    // move time forward enough to trigger a timeout.
    let now = now + Duration::from_secs(1);
    assert!(fut.poll(now).is_none());

    let request = {
        alice.advance_clock(now);
        let event = alice.pop_event().unwrap();
        match &*event {
            Event::Transmit(datagram) => datagram.borrow().to_vec(),
            e => panic!("got unanticipated event `{:?}`", e),
        }
    };

    debug!("???");
    assert!(request.len() >= ethernet2::MIN_PAYLOAD_SIZE);

    // bob hasn't heard of alice before, so he will ignore the request.
    info!("passing ARP request to bob (should be ignored)...");
    match bob.receive(&request) {
        Err(Fail::Ignored { .. }) => (),
        x => panic!("expected Fail::Ignored {{}}, got `{:?}`", x),
    }
    let cache = bob.export_arp_cache();
    assert!(cache.get(test::alice_ipv4_addr()).is_none());

    carrie.receive(&request).unwrap();
    info!("passing ARP request to carrie...");
    let cache = carrie.export_arp_cache();
    assert_eq!(
        cache.get(test::alice_ipv4_addr()),
        Some(test::alice_link_addr())
    );
    let reply = {
        carrie.advance_clock(now);
        let event = carrie.pop_event().unwrap();
        match &*event {
            Event::Transmit(datagram) => datagram.borrow().to_vec(),
            e => panic!("got unanticipated event `{:?}`", e),
        }
    };

    info!("passing ARP reply back to alice...");
    alice.receive(&reply).unwrap();
    debug!(
        "ARP cache contains: \n{}",
        serde_yaml::to_string(&alice.export_arp_cache()).unwrap()
    );
    let now = now + Duration::from_micros(1);
    let link_addr = fut.poll(now).unwrap().unwrap();
    assert_eq!(*test::carrie_link_addr(), link_addr);
}

#[test]
fn no_reply() {
    // tests to ensure that an are request results in a reply.
    let mut now = Instant::now();
    let alice = test::new_alice(now);
    let options = alice.options();

    assert_eq!(options.arp.retry_count, 2);
    assert_eq!(options.arp.request_timeout, Duration::from_secs(1));

    let fut = alice.arp_query(*test::carrie_ipv4_addr());

    alice.advance_clock(now);
    match &*alice.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let frame = ethernet2::Frame::attach(bytes.as_slice()).unwrap();
            let arp = ArpPdu::read(&mut Cursor::new(frame.text())).unwrap();
            assert_eq!(arp.op, ArpOp::ArpRequest);
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    for i in 0..options.arp.retry_count {
        now += options.arp.request_timeout;
        assert!(fut.poll(now).is_none());
        info!("no_reply(): retry #{}", i + 1);
        now += Duration::from_micros(1);
        alice.advance_clock(now);
        match &*alice.pop_event().unwrap() {
            Event::Transmit(bytes) => {
                let bytes = bytes.borrow().to_vec();
                let frame =
                    ethernet2::Frame::attach(bytes.as_slice()).unwrap();
                let arp =
                    ArpPdu::read(&mut Cursor::new(frame.text())).unwrap();
                assert_eq!(arp.op, ArpOp::ArpRequest);
            }
            e => panic!("got unanticipated event `{:?}`", e),
        }
    }

    // timeout
    now += options.arp.request_timeout;
    assert!(fut.poll(now).is_none());
    now += Duration::from_micros(1);
    match fut.poll(now).unwrap() {
        Err(Fail::Timeout {}) => (),
        x => panic!("expected Fail::Timeout {{}}, got `{:?}`", x),
    }
}
