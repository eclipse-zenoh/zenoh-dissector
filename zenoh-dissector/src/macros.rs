macro_rules! impl_for_enum {
    (
        enum $enum_name:ident {
            $(
                $variant_name:ident($variant_ty:ty),
            )*
        }
    ) => {
        impl Registration for $enum_name {
            fn generate_hf_map(prefix: &str) -> HeaderFieldMap {
                let mut hf_map = HeaderFieldMap::new()
                    .add(prefix.to_string(), stringify!{$enum_name}, FieldKind::Branch);
                $(
                    hf_map.extend(<$variant_ty>::generate_hf_map(&format!("{prefix}.{}", stringify!{$variant_name}.to_case(Case::Snake))));
                )*
                hf_map
            }

            fn generate_subtree_names(prefix: &str) -> Vec<String> {
                let mut names = vec![];
                $(
                    names.extend(<$variant_ty>::generate_subtree_names(&format!("{prefix}.{}", stringify!{$variant_name}.to_case(Case::Snake))));
                )*
                names
            }
        }

        impl AddToTree for $enum_name {
            fn add_to_tree(&self, prefix: &str, args: &TreeArgs) -> Result<()> {
                match self {
                    $(
                        Self::$variant_name(body) => {
                            body.add_to_tree(
                                &format!("{prefix}.{}", stringify!{$variant_name}.to_case(Case::Snake)),
                                &args.make_subtree(prefix, &format!("{} ({})", stringify!{$enum_name}, stringify!{$variant_name}))?
                            )?;
                        }
                    )*
                    // _ => {
                    //     let raw_txt = format!("{:?}", &self);
                    //     let msg = raw_txt.split('(').next().unwrap().split('{').next().unwrap();
                    //     bail!("Not yet implemented for {}", msg)
                    // }
                }
                Ok(())
            }
        }
    };
}

macro_rules! impl_for_struct {
    (
        struct $struct_name:ident {
            $(
                $field_name:ident: $field_ty:ty,
            )*

            $(
                #[dissect(expand)]
                $expand_name:ident: $expand_ty:ty,
            )*

            $(
                #[dissect(vec)]
                $vec_name:ident: Vec<$vec_ty:ty>,
            )*

            $(
                #[dissect(option)]
                $skip_name:ident: Option<$option_ty:ty>,
            )*

            $(
                #[dissect(enum)]
                $enum_name:ident: $enum_ty:ty,
            )*
        }
    ) => {
        impl Registration for $struct_name {
            #![allow(unused)]
            fn generate_hf_map(prefix: &str) -> HeaderFieldMap {
                let mut hf_map = HeaderFieldMap::new()
                    // Register the struct's own prefix so it is usable as a
                    // Wireshark display filter (e.g. zenoh.body.keep_alive).
                    .add(prefix.to_string(), stringify!{$struct_name}, FieldKind::Branch)
                    $(
                        .add(
                            format!("{}.{}", prefix, stringify!{$field_name}),
                            &stringify!{$field_name}.to_case(Case::Title),
                            FieldKind::Text
                        )
                    )*
                    $(
                        .add(
                            format!("{}.{}", prefix, stringify!{$vec_name}),
                            stringify!{$vec_ty},
                            FieldKind::Branch
                        )
                    )*
                    $(
                        .add(
                            format!("{}.{}", prefix, stringify!{$enum_name}),
                            stringify!{$enum_ty},
                            FieldKind::Text
                        )
                    )*
                    ;

                // recursive
                $(
                    hf_map.extend(<$vec_ty>::generate_hf_map(&format!("{prefix}.{}", stringify!{$vec_name})));
                )*

                $(
                    hf_map.extend(<$expand_ty>::generate_hf_map(&format!("{prefix}.{}", stringify!{$expand_name})));
                )*

                hf_map
            }

            fn generate_subtree_names(prefix: &str) -> Vec<String> {
                let mut names = vec![];
                // recursive
                $(
                    names.extend(<$vec_ty>::generate_subtree_names(&format!("{prefix}.{}", stringify!{$vec_name})));
                )*
                $(
                    // Structs with an expand field create their own subtree via
                    // make_subtree, so they need an ETT registered for their prefix.
                    names.push(prefix.to_string());
                    names.push(format!("{prefix}.{}", stringify!{$expand_name}));
                    names.extend(<$expand_ty>::generate_subtree_names(&format!("{prefix}.{}", stringify!{$expand_name})));
                )*
                names
            }
        }

        impl AddToTree for $struct_name {
            #![allow(unused)]
            fn add_to_tree(&self, prefix: &str, args: &TreeArgs) -> Result<()> {
                // `_has_expand` is set to true inside the expand repetition below.
                // It drives whether we emit a standalone presence label (non-expand
                // structs like KeepAlive) or let make_subtree serve as the only
                // visible node (expand structs like TransportMessage).
                let mut _has_expand = false;
                // For expand structs `_expand_args` is replaced with the struct's
                // own subtree args; for non-expand structs it stays as the parent's
                // args so fields land in the right place.
                let mut _expand_args = *args;

                // --- Phase 1: build the struct's own subtree (expand only) --------
                $(
                    // Reference $expand_name so Rust accepts this as a valid
                    // repetition of the expand group.
                    let _ = stringify!($expand_name);
                    _has_expand = true;
                    _expand_args = args.make_subtree(prefix, stringify!{$struct_name})?;
                )*

                // --- Phase 2: presence label (non-expand only) --------------------
                // For structs without an expand field (e.g. KeepAlive) this FT_NONE
                // item is the only thing added to the tree, and it is what makes
                // display filters like `zenoh.body.keep_alive` work.
                // For expand structs the make_subtree call above already created a
                // filterable, collapsible node so we skip this to avoid a duplicate.
                if !_has_expand {
                    let hf_index = args.get_hf(prefix)?;
                    unsafe {
                        epan_sys::proto_tree_add_none_format(
                            args.tree,
                            hf_index,
                            args.tvb,
                            args.start as _,
                            args.length as _,
                            nul_terminated_str(stringify!{$struct_name})?,
                        );
                    }
                }

                // --- Phase 3: regular fields --------------------------------------
                // Written into _expand_args.tree which is either the struct's own
                // subtree (expand structs) or the parent's subtree (non-expand).
                $(
                    let hf_index = _expand_args.get_hf(&format!("{prefix}.{}", stringify!{$field_name}))?;
                    unsafe {
                        epan_sys::proto_tree_add_string(
                            _expand_args.tree,
                            hf_index,
                            _expand_args.tvb,
                            _expand_args.start as _,
                            _expand_args.length as _,
                            nul_terminated_str(&format!("{:?}", self.$field_name))?,
                        );
                    }
                )*

                // --- Phase 4: vec items -------------------------------------------
                $(
                    for item in &self.$vec_name {
                        item.add_to_tree(
                            &format!("{prefix}.{}", stringify!{$vec_name}),
                            &_expand_args,
                        )?;
                    }
                )*

                // --- Phase 5: expand body (inside the struct's own subtree) -------
                $(
                    self.$expand_name.add_to_tree(
                        &format!("{prefix}.{}", stringify!{$expand_name}),
                        &_expand_args,
                    )?;
                )*

                Ok(())
            }
        }
    };
}

pub(crate) use impl_for_enum;
pub(crate) use impl_for_struct;
