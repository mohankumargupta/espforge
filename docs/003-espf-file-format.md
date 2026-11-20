# The ESPF file format

An `.espf` file is a small, line-oriented document that lists the hardware a
project uses and the software services it runs. It is *not* XML or YAML: it is
Markdown-style `#` section headers plus XML-style self-closing tags.

The syntax is implemented in [`espf/src/parser.rs`](../espf/src/parser.rs) and
[`espf/src/value.rs`](../espf/src/value.rs). Which tags and attributes are legal
in each section is decided by the schema in
[`espforge-schema/src/espf.schema.kdl`](../espforge-schema/src/espf.schema.kdl),
so this document describes the *shape* of the language, not the catalogue of
elements.

## A complete file

```xml
==================
ILI9341 TFT EXAMPLE
==================

# espforge
  <project name="ili9341" />
  <chip type="esp32c3" />
  <runtime type="blocking" />

# peripherals
  <spi id="spi2" sclk="8" mosi="7" mode="0" frequency="40000kHz" />
  <gpio id="gpio4" pin="4" direction="output" />
  <gpio id="gpio5" pin="5" direction="output" />
  <gpio id="gpio6" pin="6" direction="output" />

# devices
  <ili9341 id="tft" bus="$spi2" cs="9" dc="$gpio4" reset="$gpio5" backlight="$gpio6" rotation="90" /> // optional trailing comment

# services
```

Every file needs all four sections. A section may be empty, but the `#` header
must be there; `# services` above is the usual example of an empty one.

## Sections

A section header is a line containing `#`, optional spaces or tabs, and one
word:

```
#espforge
# peripherals
#   devices
# services      // ← not allowed: nothing may follow the name
```

* The name is an identifier: ASCII letters, digits, `_` and `-`.
* The four sections are `espforge`, `peripherals`, `devices` and `services`.
  Their order is free, but each may appear only once. A repeated section is an
  error, and the first definition wins.
* An unknown section name is an error (`E0105`). The old `# components` block is
  gone — that middle layer no longer exists. Buses are declared in
  `# peripherals` and devices bind to them directly with `bus="$id"`.
* Everything after a header, up to the next header or the end of the file,
  belongs to that section.

| Section | Holds |
|---|---|
| `# espforge` | Project-wide settings: `<project>`, `<chip>`, `<runtime>`, and optional `<console>`, `<ota>`, `<logger>`, `<options>`. |
| `# peripherals` | The chip blocks you use: `<gpio>`, `<i2c>`, `<spi>`, `<uart>`, `<wifi>`. |
| `# devices` | Drivers for parts on those buses: `<led>`, `<button>`, `<ili9341>`, `<scd4x>`, … |
| `# services` | Software on top of the hardware: `<wifi_station>`, `<http_client>`, `<ntp>`, … |

## Elements

An element is a self-closing tag and must be the only thing on its line
(indentation is ignored):

```xml
  <gpio id="gpio4" pin="4" direction="output" />
```

* Leading whitespace is fine; blanks lines between elements are fine.
* The `/>` closes it. There is no end tag, no nesting and no text content.
* Attributes may be spread over several lines, as long as the tag as a whole
  fills its lines:

  ```xml
  <wifi id="wifi"
      network1.ssid="boo" network1.password="123"
      network2.ssid="moo" network2.password="" />
  ```

* After `/>` only spaces/tabs, a newline, or an inline comment may follow on the
  same line. Anything else makes the line a syntax error (`E0101`) rather than
  being silently ignored.

## Attributes

An attribute is `key="value"`, or `key+="value"` for a list. Attribute names are
identifiers joined by dots; `=` or `+=` must follow the key immediately, and the
value must be double-quoted:

```xml
<uart id="uart1" tx="20" rx="21" baud_rate="9600" />
<ntp id="ntp" timezone="Melbourne/Australia"
     servers+="0.pool.ntp.org"
     servers+="1.pool.ntp.org" />
```

Values are raw text between the quotes:

* A value may not span lines, and a `"` always ends it. There are **no escape
  sequences**: `\"` is a backslash followed by the end of the value, and
  `&quot;` is the literal text `&quot;`. A value therefore cannot contain a
  double quote.
* Single quotes are ordinary characters, so `'` is fine inside a value.
* Backslashes, `$`, `//`, `#`, `>` and `=` are ordinary characters too.
* `//` inside a value is not a comment; comments are recognised only after
  `/>`.
* Every attribute is a string. The schema then reads it as a number
  (`pin="4"`), a boolean (exactly `"true"` or `"false"`), a frequency
  (`frequency="400kHz"`), an enum choice, a pin, or a `$` reference.

### Value shapes

| Written as | Becomes | Example |
|---|---|---|
| `key="v"` | string | `pin="18"` |
| `key+="v"` | array of strings | `servers+="0.pool.ntp.org"` |
| `a.b="v"` | nested object `{ a: { b: "v" } }` | `network1.ssid="boo"` |

The dotted form nests to any depth (`a.b.c="v"`). Rules:

* An array must use `+=` on **every** entry, including the first. Starting with
  `servers="x"` and continuing with `servers+="y"` is an error: at one path,
  `=` and `+=` cannot be mixed.
* A scalar key may be set only once. Repeating `pin="4"` twice is an error;
  repeating an array entry is legal.
* A name cannot be both a scalar and an object: `a="1"` together with
  `a.b="2"` is an error.
* Keys are matched literally and are case-sensitive.

Whether an element accepts a nested object or an array is up to its schema
entry — the shape rules above apply whenever it does.

## Comments

A block comment is two lines of `=` with the comment in between:

```
====================
NTP CLIENT EXAMPLE

Connects to Wi-Fi, then synchronises the clock.
====================
```

* After optional indentation, the opening line starts with `=`. Any text after
  the `=`s is a title, so `==== BLINK ====` opens a block.
* The closing line begins in column 1 and contains only `=` characters
  (trailing spaces/tabs are allowed).
* Blank lines are allowed inside the block. An unterminated block runs to the
  end of the file and is accepted.
* The parser treats a block as trivia wherever a blank line is allowed — before
  a section, between elements, even between a header and its first element. By
  convention, put it directly above the section (or group of elements) it
  describes.

An inline comment is `//`, and only after an element's `/>`:

```xml
  <spi id="spi2" sclk="6" mosi="7" /> // displays generally omit miso
```

`//` on a section header line, inside an attribute value, or on a line of its
own is **not** a comment.

## References

An element can be referenced by its `id` from anywhere in the document, using
`$id`:

```xml
# peripherals
  <i2c id="i2c0" sda="6" scl="7" />

# devices
  <scd4x id="air" bus="$i2c0" address="0x62" />
```

* `id` is an ordinary attribute; the schema marks it required on elements that
  other elements refer to. Give each element a distinct id.
* The reference is checked against the target element: it must exist, and some
  attributes also check its kind or direction (a `<led gpio="$gpio18">` needs a
  `<gpio>`; a bus attribute needs a bus of the declared type).
* Some attributes accept either a raw pin number or a reference — for example
  `cs="9"` or `cs="$gpio9"`, and `pin="18"` vs `gpio="$gpio18"` on an `<led>`.

## Substitutions: `env:NAME`

Any attribute value — including `id` — may be written exactly `env:NAME`. Before
validation, `espforge` replaces it with `NAME` from `substitutions.env` beside
the `.espf`, or from the process environment; the environment wins. Substitution
is a single pass, so a value that itself reads `env:OTHER` stays literal text.

```xml
<wifi_station id="home" wifi="$radio"
    ssid="env:WIFI_SSID" password="env:WIFI_PASSWORD" auth="wpa2" />
```

`env:` with no name, or `env: NAME` with a space, is ordinary literal text. See
the [README](../README.md) for the file's `KEY=VALUE` rules and git-ignore
guidance.

## Grammar

This is the accepted grammar, matching the parser (and superseding the older
[`espf.ebnf`](../espf.ebnf) at the repository root):

```ebnf
document        = { trivia | section } ;
trivia          = blank_line | comment_block ;

section         = section_header , line_end , { trivia | element } ;
section_header  = "#" , hspace , identifier ;

element         = hspace , "<" , element_name ,
                  { whitespace , attribute } ,
                  [ whitespace ] , "/>" ,
                  [ hspace , inline_comment ] ,
                  line_end ;
element_name    = identifier ;

attribute       = attr_key , ( "=" | "+=" ) , string ;
attr_key        = identifier , { "." , identifier } ;
string          = '"' , { any_char - ( '"' | newline ) } , '"' ;

comment_block   = hspace , "=" , { any_char - newline } , line_end ,
                  { comment_line } ,
                  "=" , { "=" } , line_end ;
comment_line    = { any_char - newline } , line_end ;

inline_comment  = "//" , { any_char - newline } ;

identifier      = alpha_num , { alpha_num | "_" | "-" } ;
alpha_num       = letter | digit ;

whitespace      = hspace | newline ;        (* separates attributes *)
blank_line      = hspace , newline ;
hspace          = { " " | "\t" } ;
line_end        = hspace , ( newline | end_of_file ) ;
newline         = "\r\n" | "\n" | "\r" ;
end_of_file     = ? end of input ? ;
any_char        = ? any character except the excluded one ? ;
```

A tag starts its own line because `trivia` consumes the blank lines between
elements; the continuation lines of a multi-line tag are part of `element`.
`<led/>`, `<led />` and a file that ends without a final newline are all
accepted. Anything outside this grammar — an unknown section, a missing
section, a malformed tag line — is reported as a diagnostic, never quietly
dropped.

## Diagnostics

| Code | Meaning |
|---|---|
| `E0101` | A line in a section that is not a valid element line. |
| `E0102` | A line that was expected to be a `# section` header. |
| `E0103` | The same section appears twice; the first is kept. |
| `E0104` | A required section is missing (it may still be empty). |
| `E0105` | A section outside the four-section grammar, such as `# components`. |

Unknown tags (`E0201`), unknown attributes (`E0316`), wrong value types
(`E0311`–`E0313`) and duplicate or conflicting keys (`E0314`) are reported by
the schema and value layers after parsing.

## Differences from the older notes

`format.espf.txt` and `espf.ebnf` at the repository root predate parts of the
current parser; this page is the up-to-date description. In particular:

* there are four sections, and `# components` is rejected — devices refer
  straight to the bus declared in `# peripherals`;
* the EBNF omitted `# services`, `+=` lists, dotted attribute keys, `-` in
  identifiers, whitespace/newlines between attributes, and title text on a block
  comment's opening line;
* block comments are accepted anywhere blanks are allowed, not only immediately
  before a section;
* `#espforge` and indented headers are accepted, and malformed lines are
  errors rather than skipped.
