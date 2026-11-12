//! Prints the licences of the grammars built in for colouring code, for the installer's THIRD-PARTY-NOTICES.txt.
//! `tools/gen_notices.py` runs this. The two-face library keeps them, and lists those that ask to be acknowledged.

fn main() {
    let listing = two_face::acknowledgement::listing();
    for grammar in listing.for_syntaxes() {
        println!("{}\n\n{}\n\n----\n", grammar.rel_path.display(), grammar.text.trim());
    }
    println!("The assembler grammars for 80x86, 680x0, 6502 and Z80 were written for Limoniter, and are covered by its own licence.");
}
