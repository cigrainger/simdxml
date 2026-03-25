#!/usr/bin/env python3
"""Generate benchmark XML files of various sizes and shapes."""
import os
import random
import string

DIR = os.path.dirname(os.path.abspath(__file__))

def random_text(n):
    return ''.join(random.choices(string.ascii_lowercase + ' ', k=n))

def gen_patent_corpus(target_kb):
    """Generate patent-like XML (~real-world use case)."""
    patents = []
    i = 0
    while True:
        i += 1
        claims = '\n'.join(
            f'    <claim num="{j}" type="{"independent" if j==1 else "dependent"}">'
            f'A method comprising step {j} of processing data through a neural network '
            f'layer with {random.randint(10,1000)} parameters and backpropagation.</claim>'
            for j in range(1, random.randint(3, 12))
        )
        patent = f"""  <patent id="US{10000000+i}B2" country="US">
    <title>Method for {random_text(30)}</title>
    <abstract>{random_text(200)}</abstract>
    <description>
      <p>{random_text(500)}</p>
      <p>{random_text(500)}</p>
      <p>{random_text(500)}</p>
    </description>
    <claims>
{claims}
    </claims>
    <citations>
      <ref country="US" num="{random.randint(5000000,9999999)}" kind="A"/>
      <ref country="EP" num="{random.randint(1000000,3999999)}" kind="A1"/>
    </citations>
  </patent>"""
        patents.append(patent)
        total = sum(len(p) for p in patents) + 100
        if total >= target_kb * 1024:
            break
    return f'<?xml version="1.0" encoding="UTF-8"?>\n<corpus>\n' + '\n'.join(patents) + '\n</corpus>'

def gen_deep_nested(target_kb):
    """Generate deeply nested XML (stress-tests depth tracking)."""
    lines = ['<?xml version="1.0"?>', '<root>']
    depth = 1
    size = 0
    target = target_kb * 1024
    i = 0
    while size < target:
        i += 1
        if depth < 50 and random.random() < 0.6:
            tag = f'n{depth}'
            lines.append('  ' * depth + f'<{tag} id="{i}">')
            depth += 1
        elif depth > 1:
            depth -= 1
            tag = f'n{depth}'
            lines.append('  ' * depth + f'</{tag}>')
        else:
            lines.append(f'  <item key="{i}">{random_text(80)}</item>')
        size = sum(len(l) for l in lines)
    while depth > 1:
        depth -= 1
        lines.append('  ' * depth + f'</n{depth}>')
    lines.append('</root>')
    return '\n'.join(lines)

def gen_attribute_heavy(target_kb):
    """Generate attribute-heavy XML (many small elements with attributes)."""
    lines = ['<?xml version="1.0"?>', '<data>']
    size = 0
    target = target_kb * 1024
    i = 0
    while size < target:
        i += 1
        attrs = ' '.join(f'a{j}="{random_text(10)}"' for j in range(8))
        lines.append(f'  <record id="{i}" {attrs}/>')
        size = sum(len(l) for l in lines)
    lines.append('</data>')
    return '\n'.join(lines)

def gen_text_heavy(target_kb):
    """Generate text-heavy XML (large text nodes, few tags)."""
    lines = ['<?xml version="1.0"?>', '<book>']
    size = 0
    target = target_kb * 1024
    ch = 0
    while size < target:
        ch += 1
        paragraphs = '\n'.join(f'    <p>{random_text(1000)}</p>' for _ in range(10))
        lines.append(f'  <chapter num="{ch}">\n    <title>Chapter {ch}</title>\n{paragraphs}\n  </chapter>')
        size = sum(len(l) for l in lines)
    lines.append('</book>')
    return '\n'.join(lines)

random.seed(42)  # reproducible

sizes = {
    'small': 1,      # 1 KB
    'medium': 100,    # 100 KB
    'large': 1024,    # 1 MB
    'xlarge': 10240,  # 10 MB
}

shapes = {
    'patent': gen_patent_corpus,
    'nested': gen_deep_nested,
    'attrheavy': gen_attribute_heavy,
    'textheavy': gen_text_heavy,
}

bench_dir = os.path.join(DIR, 'bench')
os.makedirs(bench_dir, exist_ok=True)

for size_name, size_kb in sizes.items():
    for shape_name, gen_fn in shapes.items():
        fname = f'{shape_name}_{size_name}.xml'
        fpath = os.path.join(bench_dir, fname)
        xml = gen_fn(size_kb)
        with open(fpath, 'w') as f:
            f.write(xml)
        actual_kb = os.path.getsize(fpath) / 1024
        print(f'{fname}: {actual_kb:.1f} KB')
