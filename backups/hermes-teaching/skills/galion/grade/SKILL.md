---
name: grade
description: Edit a photo or a short video the operator already uploaded. Grade, crop, mask, text, trim, frames. Vision only when asked to read.
---

# Grade

The file is already on disk. The path is in the message. Do not send it to a paid API. Do not load a new model.

Automatic grade, when they say edit, color, or grade:

```
python3 /workspace/galion-light/grade/grade.py PATH --auto
```

That applies the clean look. Contrast 1.06, saturation 1.15, a light sharpen. A video longer than 90 seconds uses the first 90.

When they ask for more, add one flag. Do not ask them to drag a slider.

| They say | Flag |
|---|---|
| more color, more sharp, softer | `--sat 1.3` `--sharp 0.8` `--look soft` |
| crop to the subject | `--subject` |
| a mask, darken the background | `--mask` |
| crop numbers | `--crop x:y:w:h` |
| put words on it | `--text "the words"` |
| from 2 seconds to 10 | `--from 2 --to 10` |
| pull frames | `--frames 3` |
| what is in it | `--read` |

`--read` sends the picture, or three frames of a video, to the vision server on this box. It returns a caption, the words it can see, and the objects. Use that. Do not invent what is in the picture.

`--subject` and `--mask` also ask the vision server for the main object. If it finds none, say so.

Reply with the output path. If frames were written, list them. If it read the picture, quote the caption.
