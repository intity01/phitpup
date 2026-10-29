# Third-party data

`crates/phitpup-core/src/model.bin` is a character trigram model built by
`cargo xtask train-model` from these word lists. The lists themselves are not
in the repository; `cargo xtask fetch-corpus` downloads them into `corpus/`.

| Data | Source | License |
|---|---|---|
| Thai word frequencies (`tnc_freq.txt`, Thai National Corpus) | [PyThaiNLP](https://github.com/PyThaiNLP/pythainlp) | CC0-1.0 |
| English word list (Hunspell en, from SCOWL) | [wooorm/dictionaries](https://github.com/wooorm/dictionaries) | MIT-like; full text in [`crates/phitpup-core/src/model-en-wordlist.LICENSE.txt`](crates/phitpup-core/src/model-en-wordlist.LICENSE.txt) |

`crates/phitpup-core/src/en-words.bin`, the English words that slips of the
finger are checked against, is built by `cargo xtask english-words` from
[SCOWL](http://wordlist.aspell.net/) 2020.12.07 (levels 10 to 70, unpacked into
`corpus/scowl/`), with a few everyday words added. SCOWL is free to use, copy,
change, share and sell with its copyright notice; full text in
[`crates/phitpup-core/src/en-words.LICENSE.txt`](crates/phitpup-core/src/en-words.LICENSE.txt).

The keyboard tables in `crates/phitpup-core/src/layouts/data.rs` are read from
the layout DLLs that ship with Windows (`cargo xtask gen-layouts`).
