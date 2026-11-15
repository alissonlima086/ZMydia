# Update manifest signing

ZMydia update manifests are signed with Ed25519. The application contains only the public key; the private key is used exclusively by the tagged-release workflow.

## One-time setup

Generate the key pair in a secure local directory outside the repository:

```sh
node scripts/generate-update-key.mjs \
  --private-output /secure/location/zmydia-update-private.key \
  --public-output /secure/location/zmydia-update-public.key
```

The command refuses to overwrite either file. Back up the private file securely and never commit it or copy it into a release artifact.

Configure the GitHub repository with these exact values, without additional quotes:

- Actions secret `ZMYDIA_UPDATE_SIGNING_KEY`: contents of the private key file;
- Actions variable `ZMYDIA_UPDATE_PUBLIC_KEY`: contents of the public key file.

The private value is a base64-encoded PKCS#8 Ed25519 key. The public value is the base64 encoding of the raw 32-byte Ed25519 public key.

## Release behavior

For a tagged build, the workflow:

1. requires the public key and embeds it in the server binary;
2. verifies that the tag, workspace, binary and artifact versions agree;
3. hashes the Windows NSIS installer and Linux user archive;
4. creates `zmydia-update.json` from the tag source;
5. signs its exact bytes and publishes `zmydia-update.json.sig`;
6. refuses to publish if the private and public keys do not match.

Development builds may omit the public key, but they cannot trust a production update manifest. Rotating the key requires a normal application release containing the replacement public key before manifests are signed with the new private key.
