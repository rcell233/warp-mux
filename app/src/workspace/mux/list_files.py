import json
import os
import sys

base = os.path.realpath(os.path.expanduser(sys.argv[1] or '~'))
path = os.path.realpath(os.path.expanduser(sys.argv[2] or base))
if os.path.commonpath([base, path]) != base:
    raise ValueError('Directory is outside the base path')
entries = []
truncated = False
with os.scandir(path) as children:
    for child in children:
        if len(entries) == 2000:
            truncated = True
            break
        try:
            directory = child.is_dir()
            size = child.stat().st_size
        except OSError:
            directory, size = False, 0
        entries.append(dict(name=child.name, path=child.path, directory=directory, size=size))
entries.sort(key=lambda entry: (not entry['directory'], entry['name'].casefold()))
print('__WARP_MUX_FILES__')
print(json.dumps(dict(base=base, path=path, parent=os.path.dirname(path) if path != base else None,
                     entries=entries, truncated=truncated)))
