# Compute protocol

A compute runs models on what the seat's devices record, so agents get
something they can use: speech becomes text, a camera frame becomes a
description, a wrist band's raw signals become gestures. It may run on
another host: a weak laptop has a workstation do the work.

A compute isn't registered. hunch-host on its host serves it (see
HOST.md):

    hunch-host compute ADDR COMMAND [ARGUMENTS]

Terms:

    ADDR        a compute: local: for models hunch-host runs itself, or
                the URL of an OpenAI-compatible server it passes the work
                to, e.g. http://localhost:8080/v1
    MODEL       a model, by the id models prints: a Hugging Face repo,
                e.g. mlx-community/Qwen3-VL-4B-Instruct-4bit, with
                :QUANT where a GGUF repo has several quantizations, as
                llama.cpp names them, e.g. unsloth/Qwen3-8B-GGUF:Q4_K_M;
                or a path inside a model directory
    TYPE        the media type of INPUT, e.g. audio/wav (16-bit PCM,
                16 kHz, mono), image/png, video/mp4, text/plain; a
                sensor's samples are application/x-ndjson, one JSON object
                per line
    INPUT       what MODEL works on, on stdin, as TYPE
    PROMPT      what to do with INPUT, as text, on fd 3; a model with one
                job, e.g. speech to text, needs none

Commands:

    models              the models it can run
    run MODEL TYPE      run MODEL on INPUT, print its answer on stdout
    pull MODEL          download MODEL from Hugging Face
    remove MODEL        delete a model pull downloaded
    load MODEL          load MODEL into memory now, so the next run
                        doesn't wait for it
    unload              free the memory of the loaded model

pull, remove, load and unload are local: only; a server at a URL manages
its own models. The service keeps a loaded model between runs.

## Where local: keeps models

local: finds models in the Hugging Face cache, `$HF_HOME/hub`, by
default `~/.cache/huggingface/hub`. pull downloads there and remove
deletes from there, so a model is never downloaded twice, whichever tool
fetched it.

`~/.config/hunch/models` lists more model directories, one per line,
e.g. LM Studio's. local: only reads them. A model there is a `.gguf`
file, or a directory with a `config.json` and its weights.

`models` prints a JSON array of models, each with:

    id          the MODEL
    name        its name, as the model's files give it, or null
    inputs      the TYPEs it takes
    loaded      true if it's in memory, so the next run doesn't wait for
                it to load
    size        bytes on disk

```json
[
  {
    "id": "handy-computer/parakeet-tdt-0.6b-v3-gguf",
    "name": "Parakeet TDT 0.6B v3",
    "inputs": ["audio/wav"],
    "loaded": true,
    "size": 739508576
  },
  {
    "id": "mlx-community/Qwen3-VL-4B-Instruct-4bit",
    "name": null,
    "inputs": ["image/png", "image/jpeg", "video/mp4", "text/plain"],
    "loaded": false,
    "size": 3109773312
  }
]
```

To transcribe:

    hunch-host compute local: run handy-computer/parakeet-tdt-0.6b-v3-gguf audio/wav < speech.wav

To ask about a frame:

    hunch-host compute local: run mlx-community/Qwen3-VL-4B-Instruct-4bit image/png < frame.png 3<<'PROMPT'
    Does the person look tired?
    PROMPT

Rules:

- The compute keeps no copy of INPUT, PROMPT or the answer after it
  answers.

Exit status:

- 0: done
- 1: failed, the reason is on stderr
- 4: MODEL is unknown or doesn't take TYPE
