# Third-party data

`crates/phitpup-core/src/model.bin` is a character trigram model built by
`cargo xtask train-model` from these word lists. The lists themselves are not
in the repository; `cargo xtask fetch-corpus` downloads them into `corpus/`.

| Data | Source | License |
|---|---|---|
| Thai word frequencies (`tnc_freq.txt`, Thai National Corpus) | [PyThaiNLP](https://github.com/PyThaiNLP/pythainlp) | CC0-1.0 |
| English word list (Hunspell en, from SCOWL) | [wooorm/dictionaries](https://github.com/wooorm/dictionaries) | MIT-like; full text in [`crates/phitpup-core/src/model-en-wordlist.LICENSE.txt`](crates/phitpup-core/src/model-en-wordlist.LICENSE.txt) |

The keyboard tables in `crates/phitpup-core/src/layouts/data.rs` are read from
the layout DLLs that ship with Windows (`cargo xtask gen-layouts`).
