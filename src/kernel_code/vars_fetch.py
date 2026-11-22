# Run by Limoniter on a notebook's Python kernel to get the first rows of one variable, for the data viewer.
# NAME and LIMIT are put in by Limoniter. It prints one line of JSON between two markers.
def __limoniter_fetch(name, limit):
    import json
    v = globals()[name]
    t = type(v)
    module = t.__module__ or ''

    def cell(x):
        if x is None:
            return None
        n = type(x).__name__
        if n in ('NAType', 'NaTType'):
            return None
        if isinstance(x, bool):
            return x
        if n == 'bool_':
            return bool(x)
        if hasattr(x, 'item') and not isinstance(x, (str, bytes)):
            try:
                y = x.item()
                if isinstance(y, (int, float, bool, str)):
                    x = y
            except Exception:
                pass
        if isinstance(x, bool):
            return x
        if isinstance(x, float):
            if x != x:
                return None
            return str(x) if x in (float('inf'), float('-inf')) else x
        if isinstance(x, (int, str)):
            return x
        if isinstance(x, (dict, list, tuple)):
            return json.dumps(x, default=str)
        return str(x)

    def kind_of(letter):
        return {'i': 'int', 'u': 'int', 'f': 'real', 'b': 'bool'}.get(letter, 'unknown')

    names, kinds, rows, total = [], [], [], 0
    if t.__name__ == 'DataFrame' and module.startswith('pandas'):
        df = v
        try:
            import pandas as pd
            if not isinstance(df.index, pd.RangeIndex):
                df = df.reset_index()
        except Exception:
            df = v
        total = len(df)
        head = df.head(limit)
        names = [str(c) for c in head.columns]
        kinds = [kind_of(getattr(dt, 'kind', 'O')) for dt in head.dtypes]
        rows = [[cell(x) for x in r] for r in head.itertuples(index=False, name=None)]
    elif t.__name__ == 'DataFrame' and module.startswith('polars'):
        total = v.height
        head = v.head(limit)
        names = [str(c) for c in head.columns]
        for dt in head.dtypes:
            text = str(dt).lower()
            kinds.append('int' if text.startswith(('int', 'uint')) else 'real' if text.startswith('float') else 'bool' if text.startswith('bool') else 'unknown')
        rows = [[cell(x) for x in r] for r in head.rows()]
    elif module == 'numpy' and t.__name__ == 'ndarray':
        total = v.shape[0]
        head = v[:limit]
        names = [str(i) for i in range(v.shape[1])]
        kinds = [kind_of(head.dtype.kind)] * len(names)
        rows = [[cell(x) for x in r] for r in head.tolist()]
    elif t is list:
        total = len(v)
        taken = [x if isinstance(x, dict) else {'value': x} for x in v[:limit]]
        keys = {}
        for d in taken:
            for k in d:
                keys.setdefault(k, len(keys))
        names = [str(k) for k in keys]
        kinds = ['unknown'] * len(names)
        rows = [[cell(d.get(k)) for k in keys] for d in taken]
    elif t is dict:
        keys = list(v)
        names = [str(k) for k in keys]
        kinds = ['unknown'] * len(names)
        columns = [list(v[k]) for k in keys]
        total = max(len(c) for c in columns) if columns else 0
        rows = [[cell(c[i]) if i < len(c) else None for c in columns] for i in range(min(total, limit))]
    print('@@LIMONITER@@' + json.dumps({'columns': names, 'kinds': kinds, 'rows': rows, 'total': total}, default=str) + '@@LIMONITER@@')
try:
    __limoniter_fetch(NAME, LIMIT)
finally:
    del __limoniter_fetch
