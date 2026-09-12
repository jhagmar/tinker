# Python kit

Protocol kit for the Python language image. Python 3.12 or later. Stdlib only.

```bash
cd tinker-backend
python3 -m unittest discover -s languages/python -v
```

`decode_problem` yields an object with field access (`problem.v`). `log(x)` encodes `x` with the JSON data model. `encode` writes a `solve` return. Golden fixtures live in `languages/goldens/`.
