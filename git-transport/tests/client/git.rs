use crate::fixture_bytes;
use bstr::ByteSlice;
use git_transport::{
    client::{self, git, Transport, TransportV2Ext},
    Protocol, Service,
};
use std::{
    cell::RefCell,
    io::{BufRead, Read, Write},
    ops::Deref,
    rc::Rc,
};

#[test]
fn handshake_v1_and_request() -> crate::Result {
    let mut out = Vec::new();
    let server_response = fixture_bytes("v1/clone.response");
    let mut c = git::Connection::new(
        server_response.as_slice(),
        &mut out,
        Protocol::V1,
        "/foo.git",
        Some(("example.org", None)),
        git::ConnectMode::Daemon,
    );
    assert!(c.is_stateful(), "tcp connections are stateful");
    assert_eq!(c.to_url(), "file:///foo.git");
    let mut res = c.handshake(Service::UploadPack)?;
    assert_eq!(res.actual_protocol, Protocol::V1);
    assert_eq!(
        res.capabilities
            .iter()
            .map(|c| (c.name().to_owned(), c.value().map(ToOwned::to_owned)))
            .collect::<Vec<_>>(),
        [
            ("multi_ack", None),
            ("thin-pack", None),
            ("side-band", None),
            ("side-band-64k", None),
            ("ofs-delta", None),
            ("shallow", None),
            ("deepen-since", None),
            ("deepen-not", None),
            ("deepen-relative", None),
            ("no-progress", None),
            ("include-tag", None),
            ("multi_ack_detailed", None),
            ("symref", Some("HEAD:refs/heads/master")),
            ("object-format", Some("sha1")),
            ("agent", Some("git/2.28.0"))
        ]
        .iter()
        .map(|(n, v)| (
            n.as_bytes().as_bstr().to_owned(),
            v.map(|v| v.as_bytes().as_bstr().to_owned())
        ))
        .collect::<Vec<_>>()
    );
    let refs = res
        .refs
        .as_mut()
        .expect("v1 protocol provides refs")
        .lines()
        .flat_map(Result::ok)
        .collect::<Vec<_>>();
    assert_eq!(
        refs,
        vec![
            "808e50d724f604f69ab93c6da2919c014667bedb HEAD",
            "808e50d724f604f69ab93c6da2919c014667bedb refs/heads/master"
        ]
    );
    drop(res);

    let writer = c.request(client::WriteMode::Binary, client::MessageKind::Flush)?;
    assert_eq!(writer.into_read()?.lines().next().expect("exactly one line")?, "NAK");

    let mut writer = c.request(
        client::WriteMode::OneLFTerminatedLinePerWriteCall,
        client::MessageKind::Text(b"done"),
    )?;

    writer.write_all(b"hello")?;
    writer.write_all(b"world")?;

    let mut reader = writer.into_read()?;
    let messages = Rc::new(RefCell::new(Vec::<String>::new()));
    reader.set_progress_handler(Some(Box::new({
        let sb = messages.clone();
        move |is_err, data| {
            assert!(!is_err);
            sb.deref()
                .borrow_mut()
                .push(std::str::from_utf8(data).expect("valid utf8").to_owned())
        }
    })));
    let mut pack = Vec::new();
    reader.read_to_end(&mut pack)?;
    assert_eq!(pack.len(), 876, "we receive the whole pack\u{2026}");

    drop(reader);
    let sidebands = Rc::try_unwrap(messages).expect("no other handle").into_inner();
    assert_eq!(sidebands.len(), 6, "\u{2026}along with some status messages");
    c.close()?;

    assert_eq!(
        out.as_slice().as_bstr(),
        b"002egit-upload-pack /foo.git\0host=example.org\00000000ahello\n\
        000aworld\n\
        0009done\n0000"
            .as_bstr(),
        "it sends the correct request"
    );
    Ok(())
}

#[test]
fn handshake_v1_process_mode() -> crate::Result {
    let mut out = Vec::new();
    let server_response = fixture_bytes("v1/clone.response");
    let mut c = git::Connection::new(
        server_response.as_slice(),
        &mut out,
        Protocol::V1,
        "/foo.git",
        Some(("example.org", None)),
        git::ConnectMode::Process,
    );
    c.handshake(Service::UploadPack)?;

    assert_eq!(
        out.as_slice().as_bstr(),
        b"".as_bstr(),
        "it sends no introductory line to help the daemon start the right thing"
    );
    Ok(())
}

#[test]
fn handshake_v2_and_request() -> crate::Result {
    let mut out = Vec::new();
    let input = fixture_bytes("v2/clone.response");
    let mut c = git::Connection::new(
        input.as_slice(),
        &mut out,
        Protocol::V2,
        "/bar.git",
        Some(("example.org", None)),
        git::ConnectMode::Daemon,
    );
    assert!(
        c.is_stateful(),
        "tcp connections are stateful despite the protocol version"
    );
    let res = c.handshake(Service::UploadPack)?;
    assert_eq!(res.actual_protocol, Protocol::V2);
    assert!(
        res.refs.is_none(),
        "V2 needs a separate trip for getting refs (with additional capabilities)"
    );
    assert_eq!(
        res.capabilities
            .iter()
            .map(|c| (c.name().to_owned(), c.value().map(ToOwned::to_owned)))
            .collect::<Vec<_>>(),
        [
            ("agent", Some("git/2.28.0")),
            ("ls-refs", None),
            ("fetch", Some("shallow")),
            ("server-option", None),
            ("object-format", Some("sha1"))
        ]
        .iter()
        .map(|(k, v)| (k.as_bytes().into(), v.map(|v| v.as_bytes().into())))
        .collect::<Vec<_>>()
    );
    drop(res);

    let res = c.invoke(
        "ls-refs",
        [("agent", Some("git/2.28.0")), ("object-format", Some("sha1"))]
            .iter()
            .cloned(),
        Some(
            [
                "peel",
                "symrefs",
                "ref-prefix HEAD",
                "ref-prefix refs/heads/",
                "ref-prefix refs/tags",
            ]
            .iter()
            .map(|s| s.as_bytes().as_bstr().to_owned()),
        ),
    )?;

    assert_eq!(
        res.lines().collect::<Result<Vec<_>, _>>()?,
        vec![
            "808e50d724f604f69ab93c6da2919c014667bedb HEAD symref-target:refs/heads/master".to_string(),
            "808e50d724f604f69ab93c6da2919c014667bedb refs/heads/master".into()
        ]
    );

    let mut res = c.invoke(
        "fetch",
        [
            ("agent", Some("git/2.28.0")),
            ("something-without-value", None),
            ("object-format", Some("sha1")),
        ]
        .iter()
        .cloned(),
        Some(
            [
                "thin-pack",
                "ofs-delta",
                "want 808e50d724f604f69ab93c6da2919c014667bedb",
                "done",
            ]
            .iter()
            .map(|s| s.as_bytes().as_bstr().to_owned()),
        ),
    )?;

    let mut line = String::new();
    res.read_line(&mut line)?;
    assert_eq!(line, "packfile\n");

    let messages = Rc::new(RefCell::new(Vec::<String>::new()));
    res.set_progress_handler(Some(Box::new({
        let sb = messages.clone();
        move |is_err, data| {
            assert!(!is_err);
            sb.deref()
                .borrow_mut()
                .push(std::str::from_utf8(data).expect("valid utf8").to_owned())
        }
    })));

    let mut pack = Vec::new();
    res.read_to_end(&mut pack)?;
    assert_eq!(pack.len(), 876);

    drop(res);
    let messages = Rc::try_unwrap(messages).expect("no other handle").into_inner();
    assert_eq!(messages.len(), 4);
    c.close()?;

    assert_eq!(
        out.as_slice().as_bstr(),
        b"0039git-upload-pack /bar.git\0host=example.org\0\0version=2\00014command=ls-refs
0015agent=git/2.28.0
0017object-format=sha1
00010009peel
000csymrefs
0014ref-prefix HEAD
001bref-prefix refs/heads/
0019ref-prefix refs/tags
00000012command=fetch
0015agent=git/2.28.0
001csomething-without-value
0017object-format=sha1
0001000ethin-pack
000eofs-delta
0032want 808e50d724f604f69ab93c6da2919c014667bedb
0009done
00000000"
            .as_bstr(),
        "it sends the correct request, including the adjusted version"
    );
    Ok(())
}

#[test]
fn handshake_v2_downgrade_to_v1() -> crate::Result {
    let mut out = Vec::new();
    let input = fixture_bytes("v1/clone.response");
    let mut c = git::Connection::new(
        input.as_slice(),
        &mut out,
        Protocol::V2,
        "/bar.git",
        Some(("example.org", None)),
        git::ConnectMode::Daemon,
    );
    let res = c.handshake(Service::UploadPack)?;
    assert_eq!(res.actual_protocol, Protocol::V1);
    assert!(
        res.refs.is_some(),
        "V1 downgrades 'just happen', so we should have refs as part of the handshake"
    );
    drop(res);

    assert_eq!(c.desired_protocol_version(), Protocol::V2);
    Ok(())
}
