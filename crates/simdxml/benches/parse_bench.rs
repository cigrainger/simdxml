use criterion::{criterion_group, criterion_main, Criterion, Throughput};

fn bench_parse(c: &mut Criterion) {
    let xml = include_bytes!("../../../testdata/small.xml");
    let mut group = c.benchmark_group("parse");
    group.throughput(Throughput::Bytes(xml.len() as u64));

    group.bench_function("scalar", |b| {
        b.iter(|| {
            let _ = simdxml::parse(xml).unwrap();
        });
    });

    group.finish();
}

fn bench_xpath(c: &mut Criterion) {
    let xml = include_bytes!("../../../testdata/small.xml");
    let index = simdxml::parse(xml).unwrap();
    let compiled = simdxml::CompiledXPath::compile("//claim").unwrap();

    c.bench_function("xpath_eval", |b| {
        b.iter(|| {
            let _ = compiled.eval_text(&index).unwrap();
        });
    });
}

criterion_group!(benches, bench_parse, bench_xpath);
criterion_main!(benches);
