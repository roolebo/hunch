# Inference protocol

Inference turns what the human's devices record into something they and
their tools can use: so far, speech into text. It belongs to the
workspace, not to a seat: the workspace's namespace mounts an ENDPOINT
on `inference/`, as Plan 9 mounts a CPU server's devices. A thin laptop
mounts a workstation's server; a Mac its own models.

Terms:

    ENDPOINT    where models run: local:, the models this host runs
                itself, where its platform allows, e.g. a Mac with Metal;
                or the URL of an OpenAI-compatible server, e.g.
                http://thinkstation:8080/v1
    MODEL       a model of the ENDPOINT, by the id it gives it
    TYPE        a media type, e.g. audio/ogg, audio/wav, text/plain

## Files

Every ENDPOINT has the same files:

    inference/
      models      read: its models, as JSON
      transcribe  write a request, then read the text
      login       write the key this host logs in with; a server only

models is an array of objects:

    id          the MODEL
    name        its name, or null
    inputs      the TYPEs it takes
    outputs     the TYPEs it answers with
    loaded      true if it's in memory, so a request doesn't wait for it
                to load; null if the ENDPOINT doesn't say

A model transcribes when it takes audio/* and answers text/plain. A
server's list doesn't say what its models take in OpenAI's API, so
hunch-host takes it from the server's own fields: a model Unsloth lists
with task automatic-speech-recognition, or with transcribe among its
audio_workflows, takes what audio/transcriptions takes, Ogg first, and
answers text/plain. Of any other model, inputs and outputs are empty.

A request to transcribe is a line of JSON, then the speech:

    model       the MODEL
    type        the TYPE of the speech, one of the model's inputs

```sh
{ echo '{"model": "audiocpp-parakeet-tdt-0.6b-v3", "type": "audio/ogg"}'
  cat speech.ogg; } | hunch-9p rdwr inference/transcribe
```

## Namespace

hunch-host reads the namespace from `~/.config/hunch/namespace` each
time: lines as Plan 9's, `#` starting a comment.

    mount ENDPOINT /inference

The last mount wins; without one, local: is mounted.

```
# The workstation's Unsloth
mount http://thinkstation:8080/v1 /inference
```

Rules:

- Only the human writes the namespace, by hand or in the tray's
  settings; no file of the tree changes it. Speech goes to the ENDPOINT
  mounted only.
- An ENDPOINT keeps no copy of the speech or the text after it answers.
- Each host logs in to a server with a key of its own, kept in its OS's
  store of secrets; login takes it, and no file gives it back.

Errors:

    ENOENT      nothing serves local:, e.g. no hunch-seat runs models
    EINVAL      transcribe: the request isn't one, or the model doesn't
                take its type; login: no key
    EIO         the ENDPOINT failed; errstr says why
