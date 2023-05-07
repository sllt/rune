use core::fmt::{self, Write};

use crate::no_std::collections::HashMap;
use crate::no_std::prelude::*;
use crate::no_std::Error;

use syntect::parsing::{SyntaxReference, SyntaxSet};
use pulldown_cmark::escape::{escape_href, escape_html};
use pulldown_cmark::{CowStr, Alignment, CodeBlockKind, LinkType, Tag, Event};

pub(crate) const RUST_TOKEN: &str = "rust";
pub(crate) const RUNE_TOKEN: &str = "rune";

use Event::*;

type Result<T, E = Error> = core::result::Result<T, E>;

enum TableState {
    Head,
    Body,
}

struct Writer<'a, 'o, I> {
    syntax_set: &'a SyntaxSet,
    iter: I,
    out: &'o mut String,
    codeblock: Option<&'a SyntaxReference>,
    table_state: TableState,
    table_alignments: Vec<Alignment>,
    table_cell_index: usize,
    numbers: HashMap<CowStr<'a>, usize>,
}

impl<'a, 'o, I> Writer<'a, 'o, I>
where
    I: Iterator<Item = Event<'a>>,
{
    fn new(syntax_set: &'a SyntaxSet, iter: I, out: &'o mut String) -> Self {
        Self {
            syntax_set,
            iter,
            out,
            codeblock: None,
            table_state: TableState::Head,
            table_alignments: vec![],
            table_cell_index: 0,
            numbers: HashMap::new(),
        }
    }

    #[inline]
    fn write(&mut self, s: &str) -> Result<()> {
        self.out.write_str(s)?;
        Ok(())
    }

    fn run(mut self) -> Result<()> {
        while let Some(event) = self.iter.next() {
            match event {
                Start(tag) => {
                    self.start_tag(tag)?;
                }
                End(tag) => {
                    self.end_tag(tag)?;
                }
                Text(text) => {
                    if let Some(syntax) = self.codeblock {
                        let html = super::render_code_by_syntax(self.syntax_set, text.lines(), syntax)?;
                        self.write(&html)?;
                        return Ok(());
                    } else {
                        escape_html(&mut self.out, &text)?;
                    }
                }
                Code(text) => {
                    self.write("<code>")?;
                    escape_html(&mut self.out, &text)?;
                    self.write("</code>")?;
                }
                Html(html) => {
                    self.write(&html)?;
                }
                SoftBreak => {
                    self.write(" ")?;
                }
                HardBreak => {
                    self.write("<br />")?;
                }
                Rule => {
                    self.write("<hr />")?;
                }
                FootnoteReference(name) => {
                    let len = self.numbers.len() + 1;
                    self.write("<sup class=\"footnote-reference\"><a href=\"#")?;
                    escape_html(&mut self.out, &name)?;
                    self.write("\">")?;
                    let number = *self.numbers.entry(name).or_insert(len);
                    write!(&mut self.out, "{}", number)?;
                    self.write("</a></sup>")?;
                }
                TaskListMarker(true) => {
                    self.write("<input disabled=\"\" type=\"checkbox\" checked=\"\"/>")?;
                }
                TaskListMarker(false) => {
                    self.write("<input disabled=\"\" type=\"checkbox\"/>")?;
                }
            }
        }

        Ok(())
    }

    fn start_tag(&mut self, tag: Tag<'a>) -> Result<()> {
        match tag {
            Tag::Paragraph => {
                self.write("<p>")?;
            }
            Tag::Heading(level, id, classes) => {
                self.write("<")?;

                write!(&mut self.out, "{}", level)?;

                if let Some(id) = id {
                    self.write(" id=\"")?;
                    escape_html(&mut self.out, id)?;
                    self.write("\"")?;
                }

                let mut classes = classes.iter();

                if let Some(class) = classes.next() {
                    self.write(" class=\"")?;
                    escape_html(&mut self.out, class)?;
                    for class in classes {
                        self.write(" ")?;
                        escape_html(&mut self.out, class)?;
                    }
                    self.write("\"")?;
                }

                self.write(">")?;
            }
            Tag::Table(alignments) => {
                self.table_alignments = alignments;
                self.write("<table>")?;
            }
            Tag::TableHead => {
                self.table_state = TableState::Head;
                self.table_cell_index = 0;
                self.write("<thead><tr>")?;
            }
            Tag::TableRow => {
                self.table_cell_index = 0;
                self.write("<tr>")?;
            }
            Tag::TableCell => {
                match self.table_state {
                    TableState::Head => {
                        self.write("<th")?;
                    }
                    TableState::Body => {
                        self.write("<td")?;
                    }
                }

                match self.table_alignments.get(self.table_cell_index) {
                    Some(Alignment::Left) => {
                        self.write(" style=\"text-align: left\">")?;
                    }
                    Some(Alignment::Center) => {
                        self.write(" style=\"text-align: center\">")?;
                    }
                    Some(Alignment::Right) => {
                        self.write(" style=\"text-align: right\">")?;
                    }
                    _ => {
                        self.write(">")?;
                    }
                }
            }
            Tag::BlockQuote => {
                self.write("<blockquote>")?;
            }
            Tag::CodeBlock(kind) => {
                self.write("<pre><code class=\"language-")?;
                let (lang, syntax) = self.find_syntax(&kind);
                self.codeblock = Some(syntax);
                escape_href(&mut self.out, lang)?;
                self.write("\">")?;
            }
            Tag::List(Some(1)) => {
                self.write("<ol>")?;
            }
            Tag::List(Some(start)) => {
                self.write("<ol start=\"")?;
                write!(&mut self.out, "{}", start)?;
                self.write("\">")?;
            }
            Tag::List(None) => {
                self.write("<ul>")?;
            }
            Tag::Item => {
                self.write("<li>")?;
            }
            Tag::Emphasis => {
                self.write("<em>")?;
            },
            Tag::Strong => {
                self.write("<strong>")?;
            },
            Tag::Strikethrough => {
                self.write("<del>")?;
            },
            Tag::Link(LinkType::Email, dest, title) => {
                self.write("<a href=\"mailto:")?;
                escape_href(&mut self.out, &dest)?;
                if !title.is_empty() {
                    self.write("\" title=\"")?;
                    escape_html(&mut self.out, &title)?;
                }
                self.write("\">")?;
            }
            Tag::Link(_link_type, dest, title) => {
                self.write("<a href=\"")?;
                escape_href(&mut self.out, &dest)?;
                if !title.is_empty() {
                    self.write("\" title=\"")?;
                    escape_html(&mut self.out, &title)?;
                }
                self.write("\">")?;
            }
            Tag::Image(_link_type, dest, title) => {
                self.write("<img src=\"")?;
                escape_href(&mut self.out, &dest)?;
                self.write("\" alt=\"")?;
                self.raw_text()?;

                if !title.is_empty() {
                    self.write("\" title=\"")?;
                    escape_html(&mut self.out, &title)?;
                }

                self.write("\" />")?;
            }
            Tag::FootnoteDefinition(name) => {
                self.write("<div class=\"footnote-definition\" id=\"")?;
                escape_html(&mut self.out, &name).map_err(|_| fmt::Error)?;
                self.write("\"><sup class=\"footnote-definition-label\">")?;
                let len = self.numbers.len() + 1;
                let number = *self.numbers.entry(name).or_insert(len);
                write!(&mut self.out, "{}", number)?;
                self.write("</sup>")?;
            }
        }

        Ok(())
    }

    fn find_syntax<'input>(&mut self, kind: &'input CodeBlockKind<'input>) -> (&'input str, &'a SyntaxReference) {
        if let CodeBlockKind::Fenced(fences) = &kind {
            for token in fences.split(',') {
                let (token, lookup) = match token.trim() {
                    RUNE_TOKEN => (RUNE_TOKEN, RUST_TOKEN),
                    token => (token, token),
                };

                if let Some(syntax) = self.syntax_set.find_syntax_by_token(lookup)
                {
                    return (token, syntax);
                }
            }
        }

        let Some(syntax) = self.syntax_set.find_syntax_by_token(RUST_TOKEN) else {
            return ("text", self.syntax_set.find_syntax_plain_text());
        };

        (RUNE_TOKEN, syntax)
    }

    fn end_tag(&mut self, tag: Tag) -> Result<()> {
        match tag {
            Tag::Paragraph => {
                self.write("</p>")?;
            }
            Tag::Heading(level, _id, _classes) => {
                self.write("</")?;
                write!(&mut self.out, "{}", level)?;
                self.write(">")?;
            }
            Tag::Table(_) => {
                self.write("</tbody></table>")?;
            }
            Tag::TableHead => {
                self.write("</tr></thead><tbody>")?;
                self.table_state = TableState::Body;
            }
            Tag::TableRow => {
                self.write("</tr>")?;
            }
            Tag::TableCell => {
                match self.table_state {
                    TableState::Head => {
                        self.write("</th>")?;
                    }
                    TableState::Body => {
                        self.write("</td>")?;
                    }
                }
                self.table_cell_index += 1;
            }
            Tag::BlockQuote => {
                self.write("</blockquote>")?;
            }
            Tag::CodeBlock(..) => {
                self.codeblock = None;
            }
            Tag::List(Some(_)) => {
                self.write("</ol>")?;
            }
            Tag::List(None) => {
                self.write("</ul>")?;
            }
            Tag::Item => {
                self.write("</li>")?;
            }
            Tag::Emphasis => {
                self.write("</em>")?;
            }
            Tag::Strong => {
                self.write("</strong>")?;
            }
            Tag::Strikethrough => {
                self.write("</del>")?;
            }
            Tag::Link(_, _, _) => {
                self.write("</a>")?;
            }
            Tag::Image(_, _, _) => (),
            Tag::FootnoteDefinition(_) => {
                self.write("</div>")?;
            }
        }
        Ok(())
    }

    fn raw_text(&mut self) -> Result<()> {
        let mut nest = 0;

        while let Some(event) = self.iter.next() {
            match event {
                Start(_) => nest += 1,
                End(_) => {
                    if nest == 0 {
                        break;
                    }
                    nest -= 1;
                }
                Html(text) | Code(text) | Text(text) => {
                    escape_html(&mut self.out, &text).map_err(|_| fmt::Error)?;
                }
                SoftBreak | HardBreak | Rule => {
                    self.write(" ")?;
                }
                FootnoteReference(name) => {
                    let len = self.numbers.len() + 1;
                    let number = *self.numbers.entry(name).or_insert(len);
                    write!(&mut self.out, "[{}]", number)?;
                }
                TaskListMarker(true) => self.write("[x]")?,
                TaskListMarker(false) => self.write("[ ]")?,
            }
        }

        Ok(())
    }
}

pub(super) fn push_html<'a, I>(syntax_set: &'a SyntaxSet, s: &'a mut String, iter: I) -> Result<()>
where
    I: Iterator<Item = Event<'a>>,
{
    Writer::new(syntax_set, iter, s).run()
}