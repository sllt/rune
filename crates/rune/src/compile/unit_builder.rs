//! A single execution unit in the runestick virtual machine.
//!
//! A unit consists of a sequence of instructions, and lookaside tables for
//! metadata like function locations.

use core::fmt;

use crate::no_std::collections::HashMap;
use crate::no_std::prelude::*;
use crate::no_std::sync::Arc;

use crate::ast::{Span, Spanned};
use crate::compile::meta;
use crate::compile::{self, Assembly, AssemblyInst, ErrorKind, Item, Location, Pool, WithSpan};
use crate::hash;
use crate::query::QueryInner;
use crate::runtime::debug::{DebugArgs, DebugSignature};
use crate::runtime::unit::UnitEncoder;
use crate::runtime::{
    Call, ConstValue, DebugInfo, DebugInst, Inst, Protocol, Rtti, StaticString, Unit, UnitFn,
    VariantRtti,
};
use crate::{Context, Diagnostics, Hash, SourceId};

/// Errors that can be raised when linking units.
#[derive(Debug)]
#[allow(missing_docs)]
#[non_exhaustive]
pub enum LinkerError {
    MissingFunction {
        hash: Hash,
        spans: Vec<(Span, SourceId)>,
    },
}

impl fmt::Display for LinkerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LinkerError::MissingFunction { hash, .. } => {
                write!(f, "Missing function with hash {hash}",)
            }
        }
    }
}

impl crate::no_std::error::Error for LinkerError {}

/// Instructions from a single source file.
#[derive(Debug, Default)]
pub(crate) struct UnitBuilder {
    /// Registered re-exports.
    reexports: HashMap<Hash, Hash>,
    /// Where functions are located in the collection of instructions.
    functions: hash::Map<UnitFn>,
    /// Function by address.
    functions_rev: HashMap<usize, Hash>,
    /// A static string.
    static_strings: Vec<Arc<StaticString>>,
    /// Reverse lookup for static strings.
    static_string_rev: HashMap<Hash, usize>,
    /// A static byte string.
    static_bytes: Vec<Vec<u8>>,
    /// Reverse lookup for static byte strings.
    static_bytes_rev: HashMap<Hash, usize>,
    /// Slots used for object keys.
    ///
    /// This is used when an object is used in a pattern match, to avoid having
    /// to send the collection of keys to the virtual machine.
    ///
    /// All keys are sorted with the default string sort.
    static_object_keys: Vec<Box<[String]>>,
    /// Used to detect duplicates in the collection of static object keys.
    static_object_keys_rev: HashMap<Hash, usize>,
    /// Runtime type information for types.
    rtti: hash::Map<Arc<Rtti>>,
    /// Runtime type information for variants.
    variant_rtti: hash::Map<Arc<VariantRtti>>,
    /// The current label count.
    label_count: usize,
    /// A collection of required function hashes.
    required_functions: HashMap<Hash, Vec<(Span, SourceId)>>,
    /// Debug info if available for unit.
    debug: Option<Box<DebugInfo>>,
    /// Constant values
    constants: hash::Map<ConstValue>,
    /// Hash to identifiers.
    hash_to_ident: HashMap<Hash, Box<str>>,
}

impl UnitBuilder {
    /// Insert an identifier for debug purposes.
    pub(crate) fn insert_debug_ident(&mut self, ident: &str) {
        self.hash_to_ident.insert(Hash::ident(ident), ident.into());
    }

    /// Convert into a runtime unit, shedding our build metadata in the process.
    ///
    /// Returns `None` if the builder is still in use.
    pub(crate) fn build<S>(mut self, span: Span, storage: S) -> compile::Result<Unit<S>> {
        if let Some(debug) = &mut self.debug {
            debug.functions_rev = self.functions_rev;
            debug.hash_to_ident = self.hash_to_ident;
        }

        for (from, to) in self.reexports {
            if let Some(info) = self.functions.get(&to) {
                let info = *info;
                if self.functions.insert(from, info).is_some() {
                    return Err(compile::Error::new(
                        span,
                        ErrorKind::FunctionConflictHash { hash: from },
                    ));
                }
                continue;
            }

            if let Some(value) = self.constants.get(&to) {
                let const_value = value.clone();

                if self.constants.insert(from, const_value).is_some() {
                    return Err(compile::Error::new(
                        span,
                        ErrorKind::ConstantConflict { hash: from },
                    ));
                }

                continue;
            }

            return Err(compile::Error::new(
                span,
                ErrorKind::MissingFunctionHash { hash: to },
            ));
        }

        Ok(Unit::new(
            storage,
            self.functions,
            self.static_strings,
            self.static_bytes,
            self.static_object_keys,
            self.rtti,
            self.variant_rtti,
            self.debug,
            self.constants,
        ))
    }

    /// Insert a static string and return its associated slot that can later be
    /// looked up through [lookup_string][Unit::lookup_string].
    ///
    /// Only uses up space if the static string is unique.
    pub(crate) fn new_static_string(
        &mut self,
        span: &dyn Spanned,
        current: &str,
    ) -> compile::Result<usize> {
        let current = StaticString::new(current);
        let hash = current.hash();

        if let Some(existing_slot) = self.static_string_rev.get(&hash).copied() {
            let existing = self.static_strings.get(existing_slot).ok_or_else(|| {
                compile::Error::new(
                    span,
                    ErrorKind::StaticStringMissing {
                        hash,
                        slot: existing_slot,
                    },
                )
            })?;

            if ***existing != *current {
                return Err(compile::Error::new(
                    span,
                    ErrorKind::StaticStringHashConflict {
                        hash,
                        current: (*current).clone(),
                        existing: (***existing).clone(),
                    },
                ));
            }

            return Ok(existing_slot);
        }

        let new_slot = self.static_strings.len();
        self.static_strings.push(Arc::new(current));
        self.static_string_rev.insert(hash, new_slot);
        Ok(new_slot)
    }

    /// Insert a static byte string and return its associated slot that can
    /// later be looked up through [lookup_bytes][Unit::lookup_bytes].
    ///
    /// Only uses up space if the static byte string is unique.
    pub(crate) fn new_static_bytes(
        &mut self,
        span: &dyn Spanned,
        current: &[u8],
    ) -> compile::Result<usize> {
        let hash = Hash::static_bytes(current);

        if let Some(existing_slot) = self.static_bytes_rev.get(&hash).copied() {
            let existing = self.static_bytes.get(existing_slot).ok_or_else(|| {
                compile::Error::new(
                    span,
                    ErrorKind::StaticBytesMissing {
                        hash,
                        slot: existing_slot,
                    },
                )
            })?;

            if &**existing != current {
                return Err(compile::Error::new(
                    span,
                    ErrorKind::StaticBytesHashConflict {
                        hash,
                        current: current.to_owned(),
                        existing: existing.clone(),
                    },
                ));
            }

            return Ok(existing_slot);
        }

        let new_slot = self.static_bytes.len();
        self.static_bytes.push(current.to_owned());
        self.static_bytes_rev.insert(hash, new_slot);
        Ok(new_slot)
    }

    /// Insert a new collection of static object keys, or return one already
    /// existing.
    pub(crate) fn new_static_object_keys_iter<I>(
        &mut self,
        span: &dyn Spanned,
        current: I,
    ) -> compile::Result<usize>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let current = current
            .into_iter()
            .map(|s| s.as_ref().to_owned())
            .collect::<Box<_>>();

        self.new_static_object_keys(span, current)
    }

    /// Insert a new collection of static object keys, or return one already
    /// existing.
    pub(crate) fn new_static_object_keys(
        &mut self,
        span: &dyn Spanned,
        current: Box<[String]>,
    ) -> compile::Result<usize> {
        let hash = Hash::object_keys(&current[..]);

        if let Some(existing_slot) = self.static_object_keys_rev.get(&hash).copied() {
            let existing = self.static_object_keys.get(existing_slot).ok_or_else(|| {
                compile::Error::new(
                    span,
                    ErrorKind::StaticObjectKeysMissing {
                        hash,
                        slot: existing_slot,
                    },
                )
            })?;

            if *existing != current {
                return Err(compile::Error::new(
                    span,
                    ErrorKind::StaticObjectKeysHashConflict {
                        hash,
                        current,
                        existing: existing.clone(),
                    },
                ));
            }

            return Ok(existing_slot);
        }

        let new_slot = self.static_object_keys.len();
        self.static_object_keys.push(current);
        self.static_object_keys_rev.insert(hash, new_slot);
        Ok(new_slot)
    }

    /// Declare a new struct.
    pub(crate) fn insert_meta(
        &mut self,
        span: &dyn Spanned,
        meta: &meta::Meta,
        pool: &Pool,
        query: &mut QueryInner,
    ) -> compile::Result<()> {
        match meta.kind {
            meta::Kind::Type { .. } => {
                let hash = pool.item_type_hash(meta.item_meta.item);

                let rtti = Arc::new(Rtti {
                    hash,
                    item: pool.item(meta.item_meta.item).to_owned(),
                });

                self.constants.insert(
                    Hash::associated_function(hash, Protocol::INTO_TYPE_NAME),
                    ConstValue::String(rtti.item.to_string()),
                );

                if self.rtti.insert(hash, rtti).is_some() {
                    return Err(compile::Error::new(
                        span,
                        ErrorKind::TypeRttiConflict { hash },
                    ));
                }
            }
            meta::Kind::Struct {
                fields: meta::Fields::Empty,
                ..
            } => {
                let info = UnitFn::EmptyStruct { hash: meta.hash };

                let signature = DebugSignature::new(
                    pool.item(meta.item_meta.item).to_owned(),
                    DebugArgs::EmptyArgs,
                );

                let rtti = Arc::new(Rtti {
                    hash: meta.hash,
                    item: pool.item(meta.item_meta.item).to_owned(),
                });

                if self.rtti.insert(meta.hash, rtti).is_some() {
                    return Err(compile::Error::new(
                        span,
                        ErrorKind::TypeRttiConflict { hash: meta.hash },
                    ));
                }

                if self.functions.insert(meta.hash, info).is_some() {
                    return Err(compile::Error::new(
                        span,
                        ErrorKind::FunctionConflict {
                            existing: signature,
                        },
                    ));
                }

                self.constants.insert(
                    Hash::associated_function(meta.hash, Protocol::INTO_TYPE_NAME),
                    ConstValue::String(signature.path.to_string()),
                );

                self.debug_info_mut().functions.insert(meta.hash, signature);
            }
            meta::Kind::Struct {
                fields: meta::Fields::Unnamed(args),
                ..
            } => {
                let info = UnitFn::TupleStruct {
                    hash: meta.hash,
                    args,
                };

                let signature = DebugSignature::new(
                    pool.item(meta.item_meta.item).to_owned(),
                    DebugArgs::TupleArgs(args),
                );

                let rtti = Arc::new(Rtti {
                    hash: meta.hash,
                    item: pool.item(meta.item_meta.item).to_owned(),
                });

                if self.rtti.insert(meta.hash, rtti).is_some() {
                    return Err(compile::Error::new(
                        span,
                        ErrorKind::TypeRttiConflict { hash: meta.hash },
                    ));
                }

                if self.functions.insert(meta.hash, info).is_some() {
                    return Err(compile::Error::new(
                        span,
                        ErrorKind::FunctionConflict {
                            existing: signature,
                        },
                    ));
                }

                self.constants.insert(
                    Hash::associated_function(meta.hash, Protocol::INTO_TYPE_NAME),
                    ConstValue::String(signature.path.to_string()),
                );

                self.debug_info_mut().functions.insert(meta.hash, signature);
            }
            meta::Kind::Struct { .. } => {
                let hash = pool.item_type_hash(meta.item_meta.item);

                let rtti = Arc::new(Rtti {
                    hash,
                    item: pool.item(meta.item_meta.item).to_owned(),
                });

                self.constants.insert(
                    Hash::associated_function(hash, Protocol::INTO_TYPE_NAME),
                    ConstValue::String(rtti.item.to_string()),
                );

                if self.rtti.insert(hash, rtti).is_some() {
                    return Err(compile::Error::new(
                        span,
                        ErrorKind::TypeRttiConflict { hash },
                    ));
                }
            }
            meta::Kind::Variant {
                enum_hash,
                fields: meta::Fields::Empty,
                ..
            } => {
                let rtti = Arc::new(VariantRtti {
                    enum_hash,
                    hash: meta.hash,
                    item: pool.item(meta.item_meta.item).to_owned(),
                });

                if self.variant_rtti.insert(meta.hash, rtti).is_some() {
                    return Err(compile::Error::new(
                        span,
                        ErrorKind::VariantRttiConflict { hash: meta.hash },
                    ));
                }

                let info = UnitFn::UnitVariant { hash: meta.hash };

                let signature = DebugSignature::new(
                    pool.item(meta.item_meta.item).to_owned(),
                    DebugArgs::EmptyArgs,
                );

                if self.functions.insert(meta.hash, info).is_some() {
                    return Err(compile::Error::new(
                        span,
                        ErrorKind::FunctionConflict {
                            existing: signature,
                        },
                    ));
                }

                self.debug_info_mut().functions.insert(meta.hash, signature);
            }
            meta::Kind::Variant {
                enum_hash,
                fields: meta::Fields::Unnamed(args),
                ..
            } => {
                let rtti = Arc::new(VariantRtti {
                    enum_hash,
                    hash: meta.hash,
                    item: pool.item(meta.item_meta.item).to_owned(),
                });

                if self.variant_rtti.insert(meta.hash, rtti).is_some() {
                    return Err(compile::Error::new(
                        span,
                        ErrorKind::VariantRttiConflict { hash: meta.hash },
                    ));
                }

                let info = UnitFn::TupleVariant {
                    hash: meta.hash,
                    args,
                };

                let signature = DebugSignature::new(
                    pool.item(meta.item_meta.item).to_owned(),
                    DebugArgs::TupleArgs(args),
                );

                if self.functions.insert(meta.hash, info).is_some() {
                    return Err(compile::Error::new(
                        span,
                        ErrorKind::FunctionConflict {
                            existing: signature,
                        },
                    ));
                }

                self.debug_info_mut().functions.insert(meta.hash, signature);
            }
            meta::Kind::Variant {
                enum_hash,
                fields: meta::Fields::Named(..),
                ..
            } => {
                let hash = pool.item_type_hash(meta.item_meta.item);

                let rtti = Arc::new(VariantRtti {
                    enum_hash,
                    hash,
                    item: pool.item(meta.item_meta.item).to_owned(),
                });

                if self.variant_rtti.insert(hash, rtti).is_some() {
                    return Err(compile::Error::new(
                        span,
                        ErrorKind::VariantRttiConflict { hash },
                    ));
                }
            }
            meta::Kind::Enum { .. } => {
                self.constants.insert(
                    Hash::associated_function(meta.hash, Protocol::INTO_TYPE_NAME),
                    ConstValue::String(pool.item(meta.item_meta.item).to_string()),
                );
            }
            meta::Kind::Const { .. } => {
                let Some(const_value) = query.get_const_value(meta.hash) else {
                    return Err(compile::Error::msg(
                        span,
                        format_args!("Missing constant for hash {}", meta.hash),
                    ));
                };

                self.constants.insert(meta.hash, const_value.clone());
            }
            meta::Kind::Macro { .. } => (),
            meta::Kind::AttributeMacro { .. } => (),
            meta::Kind::Function { .. } => (),
            meta::Kind::Closure { .. } => (),
            meta::Kind::AsyncBlock { .. } => (),
            meta::Kind::ConstFn { .. } => (),
            meta::Kind::Import { .. } => (),
            meta::Kind::Module { .. } => (),
        }

        Ok(())
    }

    /// Construct a new empty assembly associated with the current unit.
    pub(crate) fn new_assembly(&self, location: Location) -> Assembly {
        Assembly::new(location, self.label_count)
    }

    /// Register a new function re-export.
    pub(crate) fn new_function_reexport(
        &mut self,
        location: Location,
        item: &Item,
        target: &Item,
    ) -> compile::Result<()> {
        let hash = Hash::type_hash(item);
        let target = Hash::type_hash(target);

        if self.reexports.insert(hash, target).is_some() {
            return Err(compile::Error::new(
                location.span,
                ErrorKind::FunctionReExportConflict { hash },
            ));
        }

        Ok(())
    }

    /// Declare a new instance function at the current instruction pointer.
    pub(crate) fn new_function(
        &mut self,
        location: Location,
        item: &Item,
        instance: Option<(Hash, &str)>,
        args: usize,
        assembly: Assembly,
        call: Call,
        debug_args: Box<[Box<str>]>,
        unit_storage: &mut dyn UnitEncoder,
    ) -> compile::Result<()> {
        tracing::trace!("instance fn: {}", item);

        let offset = unit_storage.offset();

        let info = UnitFn::Offset { offset, call, args };
        let signature = DebugSignature::new(item.to_owned(), DebugArgs::Named(debug_args));

        if let Some((type_hash, name)) = instance {
            let instance_fn = Hash::associated_function(type_hash, name);

            if self.functions.insert(instance_fn, info).is_some() {
                return Err(compile::Error::new(
                    location.span,
                    ErrorKind::FunctionConflict {
                        existing: signature,
                    },
                ));
            }

            self.debug_info_mut()
                .functions
                .insert(instance_fn, signature.clone());
        }

        let hash = Hash::type_hash(item);

        if self.functions.insert(hash, info).is_some() {
            return Err(compile::Error::new(
                location.span,
                ErrorKind::FunctionConflict {
                    existing: signature,
                },
            ));
        }

        self.constants.insert(
            Hash::associated_function(hash, Protocol::INTO_TYPE_NAME),
            ConstValue::String(signature.path.to_string()),
        );

        self.debug_info_mut().functions.insert(hash, signature);
        self.functions_rev.insert(offset, hash);
        self.add_assembly(location, assembly, unit_storage)?;
        Ok(())
    }

    /// Try to link the unit with the context, checking that all necessary
    /// functions are provided.
    ///
    /// This can prevent a number of runtime errors, like missing functions.
    pub(crate) fn link(&mut self, context: &Context, diagnostics: &mut Diagnostics) {
        for (hash, spans) in &self.required_functions {
            if self.functions.get(hash).is_none() && context.lookup_function(*hash).is_none() {
                diagnostics.error(
                    SourceId::empty(),
                    LinkerError::MissingFunction {
                        hash: *hash,
                        spans: spans.clone(),
                    },
                );
            }
        }
    }

    /// Insert and access debug information.
    fn debug_info_mut(&mut self) -> &mut DebugInfo {
        self.debug.get_or_insert_with(Default::default)
    }

    /// Translate the given assembly into instructions.
    fn add_assembly(
        &mut self,
        location: Location,
        assembly: Assembly,
        storage: &mut dyn UnitEncoder,
    ) -> compile::Result<()> {
        use core::fmt::Write;

        self.label_count = assembly.label_count;

        let base = storage.extend_offsets(assembly.labels.len());
        self.required_functions.extend(assembly.required_functions);

        for (offset, (_, labels)) in &assembly.labels {
            for label in labels {
                if let Some(jump) = label.jump() {
                    label.set_jump(storage.label_jump(base, *offset, jump));
                }
            }
        }

        for (pos, (inst, span)) in assembly.instructions.into_iter().enumerate() {
            let mut comment = String::new();

            let at = storage.offset();

            let mut labels = Vec::new();

            for label in assembly
                .labels
                .get(&pos)
                .map(|e| e.1.as_slice())
                .unwrap_or_default()
            {
                if let Some(index) = label.jump() {
                    storage.mark_offset(index);
                }

                labels.push(label.to_debug_label());
            }

            match inst {
                AssemblyInst::Jump { label } => {
                    let jump = label
                        .jump()
                        .ok_or(ErrorKind::MissingLabelLocation {
                            name: label.name,
                            index: label.index,
                        })
                        .with_span(span)?;

                    if let Err(fmt::Error) = write!(comment, "label:{}", label) {
                        return Err(compile::Error::msg(span, "Failed to write comment"));
                    }

                    storage.encode(Inst::Jump { jump }).with_span(span)?;
                }
                AssemblyInst::JumpIf { label } => {
                    let jump = label
                        .jump()
                        .ok_or(ErrorKind::MissingLabelLocation {
                            name: label.name,
                            index: label.index,
                        })
                        .with_span(span)?;

                    if let Err(fmt::Error) = write!(comment, "label:{}", label) {
                        return Err(compile::Error::msg(span, "Failed to write comment"));
                    }

                    storage.encode(Inst::JumpIf { jump }).with_span(span)?;
                }
                AssemblyInst::JumpIfOrPop { label } => {
                    let jump = label
                        .jump()
                        .ok_or(ErrorKind::MissingLabelLocation {
                            name: label.name,
                            index: label.index,
                        })
                        .with_span(span)?;

                    if let Err(fmt::Error) = write!(comment, "label:{}", label) {
                        return Err(compile::Error::msg(span, "Failed to write comment"));
                    }

                    storage.encode(Inst::JumpIfOrPop { jump }).with_span(span)?;
                }
                AssemblyInst::JumpIfNotOrPop { label } => {
                    let jump = label
                        .jump()
                        .ok_or(ErrorKind::MissingLabelLocation {
                            name: label.name,
                            index: label.index,
                        })
                        .with_span(span)?;

                    if let Err(fmt::Error) = write!(comment, "label:{}", label) {
                        return Err(compile::Error::msg(span, "Failed to write comment"));
                    }

                    storage
                        .encode(Inst::JumpIfNotOrPop { jump })
                        .with_span(span)?;
                }
                AssemblyInst::JumpIfBranch { branch, label } => {
                    let jump = label
                        .jump()
                        .ok_or(ErrorKind::MissingLabelLocation {
                            name: label.name,
                            index: label.index,
                        })
                        .with_span(span)?;

                    if let Err(fmt::Error) = write!(comment, "label:{}", label) {
                        return Err(compile::Error::msg(span, "Failed to write comment"));
                    }

                    storage
                        .encode(Inst::JumpIfBranch { branch, jump })
                        .with_span(span)?;
                }
                AssemblyInst::PopAndJumpIfNot { count, label } => {
                    let jump = label
                        .jump()
                        .ok_or(ErrorKind::MissingLabelLocation {
                            name: label.name,
                            index: label.index,
                        })
                        .with_span(span)?;

                    if let Err(fmt::Error) = write!(comment, "label:{}", label) {
                        return Err(compile::Error::msg(span, "Failed to write comment"));
                    }

                    storage
                        .encode(Inst::PopAndJumpIfNot { count, jump })
                        .with_span(span)?;
                }
                AssemblyInst::IterNext { offset, label } => {
                    let jump = label
                        .jump()
                        .ok_or(ErrorKind::MissingLabelLocation {
                            name: label.name,
                            index: label.index,
                        })
                        .with_span(span)?;

                    if let Err(fmt::Error) = write!(comment, "label:{}", label) {
                        return Err(compile::Error::msg(span, "Failed to write comment"));
                    }

                    storage
                        .encode(Inst::IterNext { offset, jump })
                        .with_span(span)?;
                }
                AssemblyInst::Raw { raw } => {
                    // Optimization to avoid performing lookups for recursive
                    // function calls.
                    let inst = match raw {
                        inst @ Inst::Call { hash, args } => {
                            if let Some(UnitFn::Offset { offset, call, .. }) =
                                self.functions.get(&hash)
                            {
                                Inst::CallOffset {
                                    offset: *offset,
                                    call: *call,
                                    args,
                                }
                            } else {
                                inst
                            }
                        }
                        inst => inst,
                    };

                    storage.encode(inst).with_span(span)?;
                }
            }

            if let Some(c) = assembly.comments.get(&pos) {
                if !comment.is_empty() {
                    comment.push_str("; ");
                }

                comment.push_str(c);
            }

            let debug = self.debug.get_or_insert_with(Default::default);

            let comment = if comment.is_empty() {
                None
            } else {
                Some(comment.into())
            };

            debug.instructions.insert(
                at,
                DebugInst::new(location.source_id, span, comment, labels),
            );
        }

        Ok(())
    }
}
