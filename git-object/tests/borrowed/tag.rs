use git_object::{
    borrowed::{Signature, Tag},
    bstr::ByteSlice,
    Kind, Sign, Time,
};

mod method {
    use crate::{borrowed::fixture_bytes, hex_to_id};
    use git_object::borrowed::Tag;
    use pretty_assertions::assert_eq;

    #[test]
    fn target() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = fixture_bytes("tag", "signed.txt");
        let tag = Tag::from_bytes(&fixture)?;
        assert_eq!(tag.target(), hex_to_id("ffa700b4aca13b80cb6b98a078e7c96804f8e0ec"));
        assert_eq!(tag.target, "ffa700b4aca13b80cb6b98a078e7c96804f8e0ec".as_bytes());
        Ok(())
    }
}

mod from_bytes {
    use crate::{borrowed::fixture_bytes, borrowed::signature, borrowed::tag::tag_fixture};
    use git_object::{borrowed::Tag, bstr::ByteSlice, Kind};

    #[test]
    fn signed() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(Tag::from_bytes(&fixture_bytes("tag", "signed.txt"))?, tag_fixture(9000));
        Ok(())
    }

    #[test]
    fn empty() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            Tag::from_bytes(&fixture_bytes("tag", "empty.txt"))?,
            Tag {
                target: b"01dd4e2a978a9f5bd773dae6da7aa4a5ac1cdbbc".as_bstr(),
                name: b"empty".as_bstr(),
                target_kind: Kind::Commit,
                message: b"".as_bstr(),
                signature: Some(signature(1_592_381_636)),
                pgp_signature: None
            }
        );
        Ok(())
    }

    #[test]
    fn with_newlines() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            Tag::from_bytes(&fixture_bytes("tag", "with-newlines.txt"))?,
            Tag {
                target: b"ebdf205038b66108c0331aa590388431427493b7".as_bstr(),
                name: b"baz".as_bstr(),
                target_kind: Kind::Commit,
                message: b"hello\n\nworld".as_bstr(),
                signature: Some(signature(1_592_311_808)),
                pgp_signature: None
            }
        );
        Ok(())
    }

    #[test]
    fn no_tagger() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            Tag::from_bytes(&fixture_bytes("tag", "no-tagger.txt"))?,
            Tag {
                target: b"c39ae07f393806ccf406ef966e9a15afc43cc36a".as_bstr(),
                name: b"v2.6.11-tree".as_bstr(),
                target_kind: Kind::Tree,
                message: b"This is the 2.6.11 tree object.

NOTE! There's no commit for this, since it happened before I started with git.
Eventually we'll import some sort of history, and that should tie this tree
object up to a real commit. In the meantime, this acts as an anchor point for
doing diffs etc under git."
                    .as_bstr(),
                signature: None,
                pgp_signature: Some(
                    b"-----BEGIN PGP SIGNATURE-----
Version: GnuPG v1.2.4 (GNU/Linux)

iD8DBQBCeV/eF3YsRnbiHLsRAl+SAKCVp8lVXwpUhMEvy8N5jVBd16UCmACeOtP6
KLMHist5yj0sw1E4hDTyQa0=
=/bIK
-----END PGP SIGNATURE-----
"
                    .as_bstr()
                )
            }
        );
        Ok(())
    }

    #[test]
    fn whitespace() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            Tag::from_bytes(&fixture_bytes("tag", "whitespace.txt"))?,
            Tag {
                target: b"01dd4e2a978a9f5bd773dae6da7aa4a5ac1cdbbc".as_bstr(),
                name: b"whitespace".as_bstr(),
                target_kind: Kind::Commit,
                message: b" \ttab\nnewline\n\nlast-with-trailer\n".as_bstr(), // odd, was created with \n\n actually
                signature: Some(signature(1_592_382_888)),
                pgp_signature: None
            }
        );
        Ok(())
    }
}

fn tag_fixture(offset: i32) -> Tag<'static> {
    Tag {
        target: b"ffa700b4aca13b80cb6b98a078e7c96804f8e0ec".as_bstr(),
        name: b"1.0.0".as_bstr(),
        target_kind: Kind::Commit,
        message: b"for the signature".as_bstr(),
        pgp_signature: Some(
            b"-----BEGIN PGP SIGNATURE-----
Comment: GPGTools - https://gpgtools.org

iQIzBAABCgAdFiEEw7xSvXbiwjusbsBqZl+Z+p2ZlmwFAlsapyYACgkQZl+Z+p2Z
lmy6Ug/+KzvzqiNpzz1bMVVAzp8NCbiEO3QGYPyeQc521lBwpaTrRYR+oHJY15r3
OdL5WDysTpjN8N5FNyfmvzkuPdTkK3JlYmO7VRjdA2xu/B6vIZLaOfAowFrhMvKo
8eoqwGcAP3rC5TuWEgzq2qhbjS4JXFLd4NLjWEFqT2Y2UKm+g8TeGOsa/0pF4Nq5
xeW4qCYR0WcQLFedbpkKHxag2GfaXKvzNNJdqYhVQssNa6BeSmsfDvlWYNe617wV
NvsR/zJT0wHb5SSH+h6QmwA7LQIQF//83Vc3aF7kv9D54r3ibXW5TjZ3WoeTUZO7
kefkzJ12EYDCFLPhHvXPog518nO8Ot46dX+okrF0/B4N3RFTvjKr7VAGTzv2D/Dg
DrD531S2F71b+JIRh641eeP7bjWFQi3tWLtrEOtjjsKPJfYRMKpYFnAO4UUJ6Rck
Z5fFXEUCO8d5WT56jzKDjmVoY01lA87O1YsP/J+zQAlc9v1k6jqeQ53LZNgTN+ue
5fJuSPT3T43pSOD1VQSr3aZ2Anc4Qu7K8uX9lkpxF9Sc0tDbeCosFLZMWNVp6m+e
cjHJZXWmV4CcRfmLsXzU8s2cR9A0DBvOxhPD1TlKC2JhBFXigjuL9U4Rbq9tdegB
2n8f2douw6624Tn/6Lm4a7AoxmU+CMiYagDxDL3RuZ8CAfh3bn0=
=aIns
-----END PGP SIGNATURE-----"
                .as_bstr(),
        ),
        signature: Some(Signature {
            name: b"Sebastian Thiel".as_bstr(),
            email: b"byronimo@gmail.com".as_bstr(),
            time: Time {
                time: 1_528_473_343,
                offset,
                sign: Sign::Plus,
            },
        }),
    }
}
