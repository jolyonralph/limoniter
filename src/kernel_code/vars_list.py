# Run by Limoniter on a notebook's Python kernel to list its variables, for the Variables panel. It prints one line of
# JSON between two markers, and leaves nothing behind in the kernel's names.
def __limoniter_list():
    import json, types, inspect, reprlib
    skip = {'In', 'Out', 'exit', 'quit', 'get_ipython'}
    scalars = (int, float, bool, str, complex, type(None))
    found = []
    for name, v in list(globals().items()):
        if name.startswith('_') or name in skip:
            continue
        if isinstance(v, types.ModuleType) or inspect.isroutine(v) or inspect.isclass(v):
            continue
        try:
            t = type(v)
            module = t.__module__ or ''
            type_name = t.__qualname__ if module in ('builtins', '__main__') else module.split('.')[0] + '.' + t.__qualname__
            kind = ''
            if t.__name__ == 'DataFrame' and (module.startswith('pandas') or module.startswith('polars')):
                kind = 'frame'
            elif module == 'numpy' and t.__name__ == 'ndarray' and v.ndim == 2:
                kind = 'array'
            elif t is list and v and all(isinstance(x, dict) for x in v[:200]):
                kind = 'rows'
            elif t is dict and v and len(v) <= 5000:
                lengths = set()
                for x in v.values():
                    if isinstance(x, (list, tuple)) or (type(x).__name__ == 'ndarray' and x.ndim == 1) or type(x).__name__ == 'Series':
                        lengths.add(len(x))
                    else:
                        lengths = None
                        break
                if lengths is not None and len(lengths) == 1 and 0 not in lengths:
                    kind = 'columns'
            size = ''
            shape = getattr(v, 'shape', None)
            if shape is not None and not isinstance(v, scalars):
                try:
                    size = ' × '.join(str(int(n)) for n in shape)
                except Exception:
                    size = ''
            elif isinstance(v, (list, tuple, dict, set, frozenset, str, bytes)):
                size = str(len(v))
            value = reprlib.repr(v) if isinstance(v, scalars) else ''
            found.append({'name': name, 'type': type_name, 'size': size, 'kind': kind, 'value': value})
        except Exception:
            continue
    print('@@LIMONITER@@' + json.dumps(found) + '@@LIMONITER@@')
try:
    __limoniter_list()
finally:
    del __limoniter_list
