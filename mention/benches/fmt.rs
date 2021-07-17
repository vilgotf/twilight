use criterion::{criterion_group, criterion_main, Criterion};
use std::{
    fmt::{Display, Write},
    num::NonZeroU64,
};
use twilight_mention::fmt::Mention;
use twilight_model::id::{ChannelId, EmojiId, RoleId, UserId};

fn format_id<T: Display>(input: &mut String, formatter: &T) {
    input.write_fmt(format_args!("{}", formatter)).unwrap();
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("format channel id", |b| {
        let mut string = String::new();
        let formatter =
            ChannelId(NonZeroU64::new(999_999_999_999_999_999).expect("non zero")).mention();

        b.iter(|| format_id(&mut string, &formatter))
    });
    c.bench_function("format emoji id", |b| {
        let mut string = String::new();
        let formatter =
            EmojiId(NonZeroU64::new(999_999_999_999_999_999).expect("non zero")).mention();

        b.iter(|| format_id(&mut string, &formatter))
    });
    c.bench_function("format role id", |b| {
        let mut string = String::new();
        let formatter =
            RoleId(NonZeroU64::new(999_999_999_999_999_999).expect("non zero")).mention();

        b.iter(|| format_id(&mut string, &formatter))
    });
    c.bench_function("format user id", |b| {
        let mut string = String::new();
        let formatter =
            UserId(NonZeroU64::new(999_999_999_999_999_999).expect("non zero")).mention();

        b.iter(|| format_id(&mut string, &formatter))
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
