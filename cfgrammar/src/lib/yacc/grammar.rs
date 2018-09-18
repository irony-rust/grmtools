// Copyright (c) 2017 King's College London
// created by the Software Development Team <http://soft-dev.org/>
//
// The Universal Permissive License (UPL), Version 1.0
//
// Subject to the condition set forth below, permission is hereby granted to any person obtaining a
// copy of this software, associated documentation and/or data (collectively the "Software"), free
// of charge and under any and all copyright rights in the Software, and any and all patent rights
// owned or freely licensable by each licensor hereunder covering either (i) the unmodified
// Software as contributed to or provided by such licensor, or (ii) the Larger Works (as defined
// below), to deal in both
//
// (a) the Software, and
// (b) any piece of software and/or hardware listed in the lrgrwrks.txt file
// if one is included with the Software (each a "Larger Work" to which the Software is contributed
// by such licensors),
//
// without restriction, including without limitation the rights to copy, create derivative works
// of, display, perform, and distribute the Software and make, use, sell, offer for sale, import,
// export, have made, and have sold the Software and the Larger Work(s), and to sublicense the
// foregoing rights on either these or other terms.
//
// This license is subject to the following condition: The above copyright notice and either this
// complete permission notice or at a minimum a reference to the UPL must be included in all copies
// or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
// BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use std::cell::RefCell;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use num_traits::{self, AsPrimitive, PrimInt, Unsigned};

use {Firsts, Grammar, RIdx, PIdx, SIdx, Symbol, TIdx};
use super::YaccKind;
use yacc::firsts::YaccFirsts;
use yacc::parser::YaccParser;

const START_RULE         : &str = "^";
const IMPLICIT_RULE      : &str = "~";
const IMPLICIT_START_RULE: &str = "^~";

use yacc::ast;
use yacc::ast::GrammarValidationError;
use yacc::parser::YaccParserError;

pub type PrecedenceLevel = u64;
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Precedence {
    pub level: PrecedenceLevel,
    pub kind:  AssocKind
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum AssocKind {
    Left,
    Right,
    Nonassoc
}

/// Representation of a `YaccGrammar`. See the [top-level documentation](../../index.html) for the
/// guarantees this struct makes about rules, tokens, productions, and symbols.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct YaccGrammar<StorageT=u32> {
    /// How many rules does this grammar have?
    rules_len: RIdx<StorageT>,
    /// A mapping from `RIdx` -> `String`.
    rule_names: Vec<String>,
    /// A mapping from `TIdx` -> `Option<String>`. Every user-specified token will have a name,
    /// but tokens inserted by cfgrammar (e.g. the EOF token) won't.
    token_names: Vec<Option<String>>,
    /// A mapping from `TIdx` -> `Option<Precedence>`
    token_precs: Vec<Option<Precedence>>,
    /// How many tokens does this grammar have?
    tokens_len: TIdx<StorageT>,
    /// The offset of the EOF token.
    eof_token_idx: TIdx<StorageT>,
    /// How many productions does this grammar have?
    prods_len: PIdx<StorageT>,
    /// Which production is the sole production of the start rule?
    start_prod: PIdx<StorageT>,
    /// A list of all productions.
    prods: Vec<Vec<Symbol<StorageT>>>,
    /// A mapping from rules to their productions. Note that 1) the order of rules is identical to
    /// that of `rule_names` 2) every rule will have at least 1 production 3) productions
    /// are not necessarily stored sequentially.
    rules_prods: Vec<Vec<PIdx<StorageT>>>,
    /// A mapping from productions to their corresponding rule indexes.
    prods_rules: Vec<RIdx<StorageT>>,
    /// The precedence of each production.
    prod_precs: Vec<Option<Precedence>>,
    /// The index of the rule added for implicit tokens, if they were specified; otherwise
    /// `None`.
    implicit_rule: Option<RIdx<StorageT>>
}

// Internally, we assume that a grammar's start rule has a single production. Since we manually
// create the start rule ourselves (without relying on user input), this is a safe assumption.

impl YaccGrammar<u32> {
    pub fn new(yacc_kind: YaccKind, s: &str) -> Result<Self, YaccGrammarError> {
        YaccGrammar::new_with_storaget(yacc_kind, s)
    }
}

impl<StorageT: 'static + PrimInt + Unsigned> YaccGrammar<StorageT> where usize: AsPrimitive<StorageT> {
    /// Takes as input a Yacc grammar of [`YaccKind`](enum.YaccKind.html) as a `String` `s` and returns a
    /// [`YaccGrammar`](grammar/struct.YaccGrammar.html) (or
    /// ([`YaccGrammarError`](grammar/enum.YaccGrammarError.html) on error).
    ///
    /// As we're compiling the `YaccGrammar`, we add a new start rule (which we'll refer to as `^`,
    /// though the actual name is a fresh name that is guaranteed to be unique) that references the
    /// user defined start rule.
    pub fn new_with_storaget(yacc_kind: YaccKind, s: &str)
                          -> Result<Self, YaccGrammarError>
    {
        let ast = match yacc_kind {
            YaccKind::Original | YaccKind::Eco => {
                let mut yp = YaccParser::new(yacc_kind, s.to_string());
                try!(yp.parse());
                let mut ast = yp.ast();
                ast.complete_and_validate()?;
                ast
            }
        };

        // Check that StorageT is big enough to hold RIdx/PIdx/SIdx/TIdx values; after these
        // checks we can guarantee that things like RIdx(ast.rules.len().as_()) are safe.
        if ast.rules.len() > num_traits::cast(StorageT::max_value()).unwrap() {
            panic!("StorageT is not big enough to store this grammar's rules.");
        }
        if ast.tokens.len() > num_traits::cast(StorageT::max_value()).unwrap() {
            panic!("StorageT is not big enough to store this grammar's tokens.");
        }
        if ast.prods.len() > num_traits::cast(StorageT::max_value()).unwrap() {
            panic!("StorageT is not big enough to store this grammar's productions.");
        }
        for p in &ast.prods {
            if p.symbols.len() > num_traits::cast(StorageT::max_value()).unwrap() {
                panic!("StorageT is not big enough to store the symbols of at least one of this grammar's productions.");
            }
        }

        let mut rule_names: Vec<String> = Vec::with_capacity(ast.rules.len() + 1);

        // Generate a guaranteed unique start rule name. We simply keep making the string longer
        // until we've hit something unique (at the very worst, this will require looping for as
        // many times as there are rules). We use the same technique later for unique end
        // token and whitespace names.
        let mut start_rule = START_RULE.to_string();
        while ast.rules.get(&start_rule).is_some() {
            start_rule += START_RULE;
        }
        rule_names.push(start_rule.clone());

        let implicit_rule;
        let implicit_start_rule;
        match yacc_kind {
            YaccKind::Original => {
                implicit_rule = None;
                implicit_start_rule = None;
            },
            YaccKind::Eco => {
                if ast.implicit_tokens.is_some() {
                    let mut n1 = IMPLICIT_RULE.to_string();
                    while ast.rules.get(&n1).is_some() {
                        n1 += IMPLICIT_RULE;
                    }
                    rule_names.push(n1.clone());
                    implicit_rule = Some(n1);
                    let mut n2 = IMPLICIT_START_RULE.to_string();
                    while ast.rules.get(&n2).is_some() {
                        n2 += IMPLICIT_START_RULE;
                    }
                    rule_names.push(n2.clone());
                    implicit_start_rule = Some(n2);
                }
                else {
                    implicit_rule = None;
                    implicit_start_rule = None;
                }
            }
        };

        for k in ast.rules.keys() {
            rule_names.push(k.clone());
        }
        let mut rules_prods:Vec<Vec<PIdx<StorageT>>> = Vec::with_capacity(rule_names.len());
        let mut rule_map = HashMap::<String, RIdx<StorageT>>::new();
        for (i, v) in rule_names.iter().enumerate() {
            rules_prods.push(Vec::new());
            rule_map.insert(v.clone(), RIdx(i.as_()));
        }

        let mut token_names: Vec<Option<String>> = Vec::with_capacity(ast.tokens.len() + 1);
        let mut token_precs: Vec<Option<Precedence>> = Vec::with_capacity(ast.tokens.len() + 1);
        for k in &ast.tokens {
            token_names.push(Some(k.clone()));
            token_precs.push(ast.precs.get(k).cloned());
        }
        let eof_token_idx = TIdx(token_names.len().as_());
        token_names.push(None);
        token_precs.push(None);
        let mut token_map = HashMap::<String, TIdx<StorageT>>::new();
        for (i, v) in token_names.iter().enumerate() {
            if let Some(n) = v.as_ref() {
               token_map.insert(n.clone(), TIdx(i.as_()));
            }
        }

        // In order to avoid fiddling about with production indices from the AST, we simply map
        // tem 1:1 to grammar indices. That means that any new productions are added to the *end*
        // of the list of productions.
        let mut prods = vec![None; ast.prods.len()];
        let mut prod_precs: Vec<Option<Option<Precedence>>> = vec![None; ast.prods.len()];
        let mut prods_rules = vec![None; ast.prods.len()];
        for astrulename in &rule_names {
            let ridx = rule_map[astrulename];
            if astrulename == &start_rule {
                // Add the special start rule which has a single production which references a
                // single rule.
                rules_prods[usize::from(rule_map[astrulename])]
                    .push(PIdx(prods.len().as_()));
                let start_prod = match implicit_start_rule {
                    None => {
                        // Add ^: S;
                        vec![Symbol::Rule(rule_map[ast.start.as_ref().unwrap()])]
                    }
                    Some(ref s) => {
                        // An implicit rule has been specified, so the special start rule
                        // needs to reference the intermediate start rule required. Therefore add:
                        //   ^: ^~;
                        vec![Symbol::Rule(rule_map[s])]
                    }
                };
                prods.push(Some(start_prod));
                prod_precs.push(Some(None));
                prods_rules.push(Some(ridx));
                continue;
            }
            else if implicit_start_rule.as_ref().map_or(false, |s| s == astrulename) {
                // Add the intermediate start rule (handling implicit tokens at the beginning of
                // the file):
                //   ^~: ~ S;
                rules_prods[usize::from(rule_map[astrulename])]
                    .push(PIdx(prods.len().as_()));
                prods.push(Some(vec![Symbol::Rule(rule_map[implicit_rule.as_ref().unwrap()]),
                                     Symbol::Rule(rule_map[ast.start.as_ref().unwrap()])]));
                prod_precs.push(Some(None));
                prods_rules.push(Some(ridx));
                continue;
            }
            else if implicit_rule.as_ref().map_or(false, |s| s == astrulename) {
                // Add the implicit rule: ~: "IMPLICIT_TOKEN_1" ~ | ... | "IMPLICIT_TOKEN_N" ~ | ;
                let implicit_prods = &mut rules_prods[usize::from(rule_map[astrulename])];
                // Add a production for each implicit token
                for t in ast.implicit_tokens.as_ref().unwrap().iter() {
                    implicit_prods.push(PIdx(prods.len().as_()));
                    prods.push(Some(vec![Symbol::Token(token_map[t]), Symbol::Rule(ridx)]));
                    prod_precs.push(Some(None));
                    prods_rules.push(Some(ridx));
                }
                // Add an empty production
                implicit_prods.push(PIdx(prods.len().as_()));
                prods.push(Some(vec![]));
                prod_precs.push(Some(None));
                prods_rules.push(Some(ridx));
                continue;
            }

            let rule = &mut rules_prods[usize::from(ridx)];
            for &prod_idx in &ast.rules[astrulename] {
                let astprod = &ast.prods[prod_idx];
                let mut prod = Vec::with_capacity(astprod.symbols.len());
                for astsym in &astprod.symbols {
                    match *astsym {
                        ast::Symbol::Rule(ref n) => {
                            prod.push(Symbol::Rule(rule_map[n]));
                        },
                        ast::Symbol::Token(ref n) => {
                            prod.push(Symbol::Token(token_map[n]));
                            if implicit_rule.is_some() {
                                prod.push(Symbol::Rule(rule_map[&implicit_rule.clone().unwrap()]));
                            }
                        }
                    };
                }
                let mut prec = None;
                if let Some(ref n) = astprod.precedence {
                    prec = Some(ast.precs[n]);
                } else {
                    for astsym in astprod.symbols.iter().rev() {
                        if let ast::Symbol::Token(ref n) = *astsym {
                            if let Some(p) = ast.precs.get(n) {
                                prec = Some(*p);
                            }
                            break;
                        }
                    }
                }
                (*rule).push(PIdx(prod_idx.as_()));
                prods[prod_idx] = Some(prod);
                prod_precs[prod_idx] = Some(prec);
                prods_rules[prod_idx] = Some(ridx);
            }
        }

        assert!(!token_names.is_empty());
        assert!(!rule_names.is_empty());
        Ok(YaccGrammar{
            rules_len:        RIdx(rule_names.len().as_()),
            rule_names,
            tokens_len:       TIdx(token_names.len().as_()),
            eof_token_idx,
            token_names,
            token_precs,
            prods_len:        PIdx(prods.len().as_()),
            start_prod:       rules_prods[usize::from(rule_map[&start_rule])][0],
            rules_prods,
            prods_rules:      prods_rules.into_iter().map(|x| x.unwrap()).collect(),
            prods:            prods.into_iter().map(|x| x.unwrap()).collect(),
            prod_precs:       prod_precs.into_iter().map(|x| x.unwrap()).collect(),
            implicit_rule: implicit_rule.and_then(|x| Some(rule_map[&x]))
        })
    }

    /// Return the index of the end token.
    pub fn eof_token_idx(&self) -> TIdx<StorageT> {
        self.eof_token_idx
    }

    /// Return the productions for rule `ridx`. Panics if `ridx` doesn't exist.
    pub fn rule_to_prods(&self, ridx: RIdx<StorageT>) -> &[PIdx<StorageT>] {
        &self.rules_prods[usize::from(ridx)]
    }

    /// Return the name of rule `ridx`. Panics if `ridx` doesn't exist.
    pub fn rule_name(&self, ridx: RIdx<StorageT>) -> &str {
        &self.rule_names[usize::from(ridx)]
    }

    /// Get the sequence of symbols for production `pidx`. Panics if `pidx` doesn't exist.
    pub fn prod(&self, pidx: PIdx<StorageT>) -> &[Symbol<StorageT>] {
        &self.prods[usize::from(pidx)]
    }

    /// How many symbols does production `pidx` have? Panics if `pidx` doesn't exist.
    pub fn prod_len(&self, pidx: PIdx<StorageT>) -> SIdx<StorageT> {
        // Since we've already checked that StorageT can store all the symbols for every production
        // in the grammar, the call to as_ is safe.
        SIdx(self.prods[usize::from(pidx)].len().as_())
    }

    /// Return the rule index of the production `pidx`. Panics if `pidx` doesn't exist.
    pub fn prod_to_rule(&self, pidx: PIdx<StorageT>) -> RIdx<StorageT>
    {
        self.prods_rules[usize::from(pidx)]
    }

    /// Return the precedence of production `pidx` (where `None` indicates "no precedence specified").
    /// Panics if `pidx` doesn't exist.
    pub fn prod_precedence(&self, pidx: PIdx<StorageT>) -> Option<Precedence> {
        self.prod_precs[usize::from(pidx)]
    }

    /// Return the name of token `tidx` (where `None` indicates "the rule has no name"). Panics if
    /// `tidx` doesn't exist.
    pub fn token_name(&self, tidx: TIdx<StorageT>) -> Option<&str> {
        self.token_names[usize::from(tidx)].as_ref().and_then(|x| Some(x.as_str()))
    }

    /// Return the precedence of token `tidx` (where `None` indicates "no precedence specified").
    /// Panics if `tidx` doesn't exist.
    pub fn token_precedence(&self, tidx: TIdx<StorageT>) -> Option<Precedence> {
        self.token_precs[usize::from(tidx)]
    }

    /// Returns a map from names to `TIdx`s of all tokens that a lexer will need to generate valid
    /// inputs from this grammar.
    pub fn tokens_map(&self) -> HashMap<&str, TIdx<StorageT>> {
        let mut m = HashMap::with_capacity(usize::from(self.tokens_len) - 1);
        for tidx in self.iter_tidxs() {
            if let Some(n) = self.token_names[usize::from(tidx)].as_ref() {
                m.insert(&**n, tidx);
            }
        }
        m
    }

    /// Return the production index of the start rule's sole production (for Yacc grammars the
    /// start rule is defined to have precisely one production).
    pub fn start_prod(&self) -> PIdx<StorageT> {
        self.start_prod
    }

    /// Return the `RIdx` of the implict rule if it exists, or `None` otherwise.
    pub fn implicit_rule(&self) -> Option<RIdx<StorageT>> {
        self.implicit_rule
    }

    /// Return the index of the rule named `n` or `None` if it doesn't exist.
    pub fn rule_idx(&self, n: &str) -> Option<RIdx<StorageT>> {
        self.rule_names.iter()
                          .position(|x| x == n)
                          // The call to as_() is safe because rule_names is guaranteed to be
                          // small enough to fit into StorageT
                          .map(|x| RIdx(x.as_()))
    }

    /// Return the index of the token named `n` or `None` if it doesn't exist.
    pub fn token_idx(&self, n: &str) -> Option<TIdx<StorageT>> {
        self.token_names.iter()
                       .position(|x| x.as_ref().map_or(false, |x| x == n))
                       // The call to as_() is safe because token_names is guaranteed to be small
                       // enough to fit into StorageT
                       .map(|x| TIdx(x.as_()))
    }

    /// Is there a path from the `from` rule to the `to` rule? Note that recursive rules
    /// return `true` for a path from themselves to themselves.
    pub fn has_path(&self, from: RIdx<StorageT>, to: RIdx<StorageT>) -> bool {
        let mut seen = vec![];
        seen.resize(usize::from(self.rules_len()), false);
        let mut todo = vec![];
        todo.resize(usize::from(self.rules_len()), false);
        todo[usize::from(from)] = true;
        loop {
            let mut empty = true;
            for ridx in self.iter_rules() {
                if !todo[usize::from(ridx)] {
                    continue;
                }
                seen[usize::from(ridx)] = true;
                todo[usize::from(ridx)] = false;
                empty = false;
                for pidx in self.rule_to_prods(ridx).iter() {
                    for sym in self.prod(*pidx) {
                        if let Symbol::Rule(p_ridx) = *sym {
                            if p_ridx == to {
                                return true;
                            }
                            if !seen[usize::from(p_ridx)] {
                                todo[usize::from(p_ridx)] = true;
                            }
                        }
                    }
                }
            }
            if empty {
                return false;
            }
        }
    }

    /// Return a `SentenceGenerator` which can then generate minimal sentences for any rule
    /// based on the user-defined `token_cost` function which gives the associated cost for
    /// generating each token (where the cost must be greater than 0). Note that multiple
    /// tokens can have the same score. The simplest cost function is thus `|_| 1`.
    pub fn sentence_generator<F>(&self, token_cost: F) -> SentenceGenerator<StorageT>
                        where F: Fn(TIdx<StorageT>) -> u8
    {
        SentenceGenerator::new(self, token_cost)
    }

    /// Return a `YaccFirsts` struct, allowing static dispatch (rather than the `firsts()` method,
    /// which returns `Box<Firsts>`).
    pub fn yacc_firsts(&self) -> YaccFirsts<StorageT> {
        YaccFirsts::new(self)
    }
}

impl<StorageT: 'static + PrimInt + Unsigned> Grammar<StorageT>
for YaccGrammar<StorageT>
where usize: AsPrimitive<StorageT>
{
    fn prods_len(&self) -> PIdx<StorageT> {
        self.prods_len
    }

    fn rules_len(&self) -> RIdx<StorageT> {
        self.rules_len
    }

    /// Return the index of the start rule.
    fn start_rule_idx(&self) -> RIdx<StorageT>
    {
        self.prod_to_rule(self.start_prod)
    }

    fn tokens_len(&self) -> TIdx<StorageT> {
        self.tokens_len
    }

    fn firsts(&self) -> Box<dyn Firsts<StorageT>> {
        Box::new(YaccFirsts::new(self))
    }
}

/// A `SentenceGenerator` can generate minimal sentences for any given rule. e.g. for the
/// grammar:
///
/// ```text
/// %start A
/// %%
/// A: A B | ;
/// B: C | D;
/// C: 'x' B | 'x';
/// D: 'y' B | 'y' 'z';
/// ```
///
/// the following are valid minimal sentences:
///
/// ```text
/// A: []
/// B: [x]
/// C: [x]
/// D: [y, x] or [y, z]
/// ```
pub struct SentenceGenerator<'a, StorageT: 'a> {
    grm: &'a YaccGrammar<StorageT>,
    rule_min_costs: RefCell<Option<Vec<u16>>>,
    rule_max_costs: RefCell<Option<Vec<u16>>>,
    token_costs: Vec<u8>
}

impl<'a, StorageT: 'static + PrimInt + Unsigned> SentenceGenerator<'a, StorageT>
where usize: AsPrimitive<StorageT>
{
    fn new<F>(grm: &'a YaccGrammar<StorageT>, token_cost: F) -> Self
        where F: Fn(TIdx<StorageT>) -> u8
    {
        let mut token_costs = Vec::with_capacity(usize::from(grm.tokens_len()));
        for tidx in grm.iter_tidxs() {
            token_costs.push(token_cost(tidx));
        }
        SentenceGenerator{grm,
                          token_costs,
                          rule_min_costs: RefCell::new(None),
                          rule_max_costs: RefCell::new(None)}
    }

    /// What is the cost of a minimal sentence for the rule `ridx`? Note that, unlike
    /// `min_sentence`, this function does not actually *build* a sentence and it is thus much
    /// faster.
    pub fn min_sentence_cost(&self, ridx: RIdx<StorageT>) -> u16 {
        self.rule_min_costs.borrow_mut()
                              .get_or_insert_with(|| rule_min_costs(self.grm,
                                                                       &self.token_costs))
                              [usize::from(ridx)]
    }

    /// What is the cost of a maximal sentence for the rule `ridx`? Rules which can generate
    /// sentences of unbounded length return None; rules which can only generate maximal strings of
    /// a finite length return a `Some(u16)`.
    pub fn max_sentence_cost(&self, ridx: RIdx<StorageT>) -> Option<u16> {
        let v = self.rule_max_costs.borrow_mut()
                                      .get_or_insert_with(||
                                           rule_max_costs(self.grm, &self.token_costs))
                                      [usize::from(ridx)];
        if v == u16::max_value() {
            None
        } else {
            Some(v)
        }
    }

    /// Non-deterministically return a minimal sentence from the set of minimal sentences for the
    /// rule `ridx`.
    pub fn min_sentence(&self, ridx: RIdx<StorageT>) -> Vec<TIdx<StorageT>> {
        let cheapest_prod = |p_ridx: RIdx<StorageT>| -> PIdx<StorageT> {
            let mut low_sc = None;
            let mut low_idx = None;
            for &pidx in self.grm.rule_to_prods(p_ridx).iter() {
                let mut sc = 0;
                for sym in self.grm.prod(pidx).iter() {
                    sc += match *sym {
                        Symbol::Rule(i) => self.min_sentence_cost(i),
                        Symbol::Token(i)    => u16::from(self.token_costs[usize::from(i)])
                    };
                }
                if low_sc.is_none() || sc < low_sc.unwrap() {
                    low_sc = Some(sc);
                    low_idx = Some(pidx);
                }
            }
            low_idx.unwrap()
        };

        let mut s = vec![];
        let mut st = vec![(cheapest_prod(ridx), 0)];
        while !st.is_empty() {
            let (pidx, sym_idx) = st.pop().unwrap();
            let prod = self.grm.prod(pidx);
            for (sidx, sym) in prod.iter().enumerate().skip(sym_idx) {
                match sym {
                    Symbol::Rule(s_ridx) => {
                        st.push((pidx, sidx + 1));
                        st.push((cheapest_prod(*s_ridx), 0));
                    },
                    Symbol::Token(s_tidx) => {
                        s.push(*s_tidx);
                    }
                }
            }
        }
        s
    }

    /// Return (in arbitrary order) all the minimal sentences for the rule `ridx`.
    pub fn min_sentences(&self, ridx: RIdx<StorageT>) -> Vec<Vec<TIdx<StorageT>>> {
        let cheapest_prods = |p_ridx: RIdx<StorageT>| -> Vec<PIdx<StorageT>> {
            let mut low_sc = None;
            let mut low_idxs = vec![];
            for &pidx in self.grm.rule_to_prods(p_ridx).iter() {
                let mut sc = 0;
                for sym in self.grm.prod(pidx).iter() {
                    sc += match *sym {
                        Symbol::Rule(s_ridx) => self.min_sentence_cost(s_ridx),
                        Symbol::Token(s_tidx) => u16::from(self.token_costs[usize::from(s_tidx)])
                    };
                }
                if low_sc.is_none() || sc <= low_sc.unwrap() {
                    if low_sc.is_some() && sc < low_sc.unwrap() {
                        low_idxs.clear();
                    }
                    low_sc = Some(sc);
                    low_idxs.push(pidx);
                }
            }
            low_idxs
        };

        let mut sts = Vec::new(); // Output sentences
        for pidx in cheapest_prods(ridx) {
            let prod = self.grm.prod(pidx);
            if prod.is_empty() {
                sts.push(vec![]);
                continue;
            }

            // We construct the minimal sentences in two phases.
            //
            // First, for each symbol in the production, we gather all the possible minimal
            // sentences for it. If, for the grammar:
            //   X: 'a' Y
            //   Y: 'b' | 'c'
            // we ask for the minimal sentences of X's only production we'll end up with a vec of
            // vecs as follows:
            //   [[['a']], [['b'], ['c']]]

            let mut ms = Vec::with_capacity(prod.len());
            for sym in prod {
                match *sym {
                    Symbol::Rule(s_ridx) => ms.push(self.min_sentences(s_ridx)),
                    Symbol::Token(s_tidx) => ms.push(vec![vec![s_tidx]])
                }
            }

            // Second, we need to generate all combinations of the gathered sentences. We do this
            // by writing our own simple numeric incrementing scheme. If we rewrite the list from
            // above as follows:
            //
            //      0 1 <- call this axis "i"
            //   0: a b
            //   1:   c
            //   ^
            //   |
            //   call this axis "todo"
            //
            // this hopefully becomes easier to see. Let's call the list "ms": the combinations we
            // need to generate are thus:
            //
            //   ms[0][0] + ms[1][0]  (i.e. 'ab')
            //   ms[0][0] + ms[1][1]  (i.e. 'ac')
            //
            // The easiest way to model this is to have a list (todo) with each entry starting at
            // 0. After each iteration around the loop (i) we add 1 to the last todo column's
            // entry: if that spills over the length of the corresponding ms entry, then we reset
            // that column to zero, and try adding 1 to the previous column (as many times as
            // needed). If the first column spills, then we're done. This is basically normal
            // arithmetic but with each digit having an arbitrary base.

            let mut todo = Vec::new();
            todo.resize(prod.len(), 0);
            let mut cur = Vec::new();
            'b: loop {
                for i in 0..todo.len() {
                    cur.extend(&ms[i][todo[i]]);
                }
                sts.push(cur.drain(..).collect::<Vec<TIdx<StorageT>>>());

                let mut j = todo.len() - 1;
                loop {
                    if todo[j] + 1 == ms[j].len() {
                        if j == 0 {
                            break 'b;
                        }
                        todo[j] = 0;
                        j -= 1;
                    } else {
                        todo[j] += 1;
                        break;
                    }
                }
            }
        }
        sts
    }
}

/// Return the cost of a minimal string for each rule in this grammar. The cost of a
/// token is specified by the user-defined `token_cost` function.
fn rule_min_costs<StorageT: 'static + PrimInt + Unsigned>
                    (grm: &YaccGrammar<StorageT>, token_costs: &[u8]) -> Vec<u16>
              where usize: AsPrimitive<StorageT>
{
    // We use a simple(ish) fixed-point algorithm to determine costs. We maintain two lists
    // "costs" and "done". An integer costs[i] starts at 0 and monotonically increments
    // until done[i] is true, at which point costs[i] value is fixed. We also use the done
    // list as a simple "todo" list: whilst there is at least one false value in done, there is
    // still work to do.
    //
    // On each iteration of the loop, we examine each rule in the todo list to see if
    // we can get a better idea of its true cost. Some are trivial:
    //   * A rule with an empty production immediately has a cost of 0.
    //   * Rules whose productions don't reference any rules (i.e. only contain tokens) can be
    //     immediately given a cost by calculating the lowest-cost production.
    // However if a rule A references another rule B, we may need to wait until
    // we've fully analysed B before we can cost A. This might seem to cause problems with
    // recursive rules, so we introduce the concept of "incomplete costs" i.e. if a production
    // references a rule we can work out its minimum possible cost simply by counting
    // the production's token costs. Since rules can have a mix of complete and
    // incomplete productions, this is sometimes enough to allow us to assign a final cost to
    // a rule (if the lowest complete production's cost is lower than or equal to all
    // the lowest incomplete production's cost). This allows us to make progress, since it
    // means that we can iteratively improve our knowledge of a token's minimum cost:
    // eventually we will reach a point where we can determine it definitively.

    let mut costs = vec![];
    costs.resize(usize::from(grm.rules_len()), 0);
    let mut done = vec![];
    done.resize(usize::from(grm.rules_len()), false);
    loop {
        let mut all_done = true;
        for i in 0..done.len() {
            if done[i] {
                continue;
            }
            all_done = false;
            let mut ls_cmplt = None; // lowest completed cost
            let mut ls_noncmplt = None; // lowest non-completed cost
            // The call to as_() is guaranteed safe because done.len() == grm.rules_len(), and
            // we guarantee that grm.rules_len() can fit in StorageT.
            for pidx in grm.rule_to_prods(RIdx(i.as_())).iter() {
                let mut c: u16 = 0; // production cost
                let mut cmplt = true;
                for sym in grm.prod(*pidx) {
                    let sc = match *sym {
                                 Symbol::Token(tidx) =>
                                     u16::from(token_costs[usize::from(tidx)]),
                                 Symbol::Rule(ridx) => {
                                     if !done[usize::from(ridx)] {
                                         cmplt = false;
                                     }
                                     costs[usize::from(ridx)]
                                 }
                             };
                    c = c.checked_add(sc).expect(
                            "Overflow occurred when calculating rule costs");
                }
                if cmplt && (ls_cmplt.is_none() || c < ls_cmplt.unwrap()) {
                    ls_cmplt = Some(c);
                } else if !cmplt && (ls_noncmplt.is_none() || c < ls_noncmplt.unwrap()) {
                    ls_noncmplt = Some(c);
                }
            }
            if ls_cmplt.is_some() && (ls_noncmplt.is_none() || ls_cmplt < ls_noncmplt) {
                debug_assert!(ls_cmplt.unwrap() >= costs[i]);
                costs[i] = ls_cmplt.unwrap();
                done[i] = true;
            } else if ls_noncmplt.is_some() {
                debug_assert!(ls_noncmplt.unwrap() >= costs[i]);
                costs[i] = ls_noncmplt.unwrap();
            }
        }
        if all_done {
            debug_assert!(done.iter().all(|x| *x));
            break;
        }
    }
    costs
}


/// Return the cost of the maximal string for each rule in this grammar (u32::max_val()
/// representing "this rule can generate strings of infinite length"). The cost of a
/// token is specified by the user-defined `token_cost` function.
fn rule_max_costs<StorageT: 'static + PrimInt + Unsigned>
                    (grm: &YaccGrammar<StorageT>, token_costs: &[u8]) -> Vec<u16>
               where usize: AsPrimitive<StorageT>
{
    let mut done = vec![];
    done.resize(usize::from(grm.rules_len()), false);
    let mut costs = vec![];
    costs.resize(usize::from(grm.rules_len()), 0);

    // First mark all recursive rules.
    for ridx in grm.iter_rules() {
        // Calling has_path so frequently is not exactly efficient...
        if grm.has_path(ridx, ridx) {
            costs[usize::from(ridx)] = u16::max_value();
            done[usize::from(ridx)] = true;
        }
    }

    loop {
        let mut all_done = true;
        for i in 0..done.len() {
            if done[i] {
                continue;
            }
            all_done = false;
            let mut hs_cmplt = None; // highest completed cost
            let mut hs_noncmplt = None; // highest non-completed cost
            // The call to as_() is guaranteed safe because done.len() == grm.rules_len(), and
            // we guarantee that grm.rules_len() can fit in StorageT.
            'a: for pidx in grm.rule_to_prods(RIdx(i.as_())).iter() {
                let mut c: u16 = 0; // production cost
                let mut cmplt = true;
                for sym in grm.prod(*pidx) {
                    let sc = match *sym {
                                 Symbol::Token(s_tidx) =>
                                     u16::from(token_costs[usize::from(s_tidx)]),
                                 Symbol::Rule(s_ridx) => {
                                     if costs[usize::from(s_ridx)] == u16::max_value() {
                                         // As soon as we find reference to an infinite rule, we
                                         // can stop looking.
                                         hs_cmplt = Some(u16::max_value());
                                         break 'a;
                                     }
                                     if !done[usize::from(s_ridx)] {
                                         cmplt = false;
                                     }
                                     costs[usize::from(s_ridx)]
                                 }
                             };
                    c = c.checked_add(sc).expect(
                            "Overflow occurred when calculating rule costs");
                    if c == u16::max_value() {
                        panic!("Unable to represent cost in 64 bits.");
                    }
                }
                if cmplt && (hs_cmplt.is_none() || c > hs_cmplt.unwrap()) {
                    hs_cmplt = Some(c);
                } else if !cmplt && (hs_noncmplt.is_none() || c > hs_noncmplt.unwrap()) {
                    hs_noncmplt = Some(c);
                }
            }
            if hs_cmplt.is_some() && (hs_noncmplt.is_none() || hs_cmplt > hs_noncmplt) {
                debug_assert!(hs_cmplt.unwrap() >= costs[i]);
                costs[i] = hs_cmplt.unwrap();
                done[i] = true;
            } else if hs_noncmplt.is_some() {
                debug_assert!(hs_noncmplt.unwrap() >= costs[i]);
                costs[i] = hs_noncmplt.unwrap();
            }
        }
        if all_done {
            debug_assert!(done.iter().all(|x| *x));
            break;
        }
    }
    costs
}

#[derive(Debug)]
pub enum YaccGrammarError {
    YaccParserError(YaccParserError),
    GrammarValidationError(GrammarValidationError)
}

impl Error for YaccGrammarError {}

impl From<YaccParserError> for YaccGrammarError {
    fn from(err: YaccParserError) -> YaccGrammarError {
        YaccGrammarError::YaccParserError(err)
    }
}

impl From<GrammarValidationError> for YaccGrammarError {
    fn from(err: GrammarValidationError) -> YaccGrammarError {
        YaccGrammarError::GrammarValidationError(err)
    }
}

impl fmt::Display for YaccGrammarError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            YaccGrammarError::YaccParserError(ref e) => e.fmt(f),
            YaccGrammarError::GrammarValidationError(ref e) => e.fmt(f)
        }
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use super::{IMPLICIT_RULE, IMPLICIT_START_RULE, rule_max_costs, rule_min_costs};
    use {Grammar, RIdx, PIdx, Symbol, TIdx};
    use yacc::{AssocKind, Precedence, YaccGrammar, YaccKind};

    #[test]
    fn test_minimal() {
        let grm = YaccGrammar::new(YaccKind::Original,
                           "%start R %token T %% R: 'T';").unwrap();

        assert_eq!(grm.start_prod, PIdx(1));
        assert_eq!(grm.implicit_rule(), None);
        grm.rule_idx("^").unwrap();
        grm.rule_idx("R").unwrap();
        grm.token_idx("T").unwrap();

        assert_eq!(grm.rules_prods, vec![vec![PIdx(1)], vec![PIdx(0)]]);
        let start_prod = grm.prod(grm.rules_prods[usize::from(grm.rule_idx("^").unwrap())][0]);
        assert_eq!(*start_prod, [Symbol::Rule(grm.rule_idx("R").unwrap())]);
        let r_prod = grm.prod(grm.rules_prods[usize::from(grm.rule_idx("R").unwrap())][0]);
        assert_eq!(*r_prod, [Symbol::Token(grm.token_idx("T").unwrap())]);
        assert_eq!(grm.prods_rules, vec![RIdx(1), RIdx(0)]);

        assert_eq!(grm.tokens_map(),
                   [("T", TIdx(0))].iter()
                                                .cloned()
                                                .collect::<HashMap<&str, TIdx<_>>>());
        assert_eq!(grm.iter_rules().collect::<Vec<_>>(),
                   vec![RIdx(0), RIdx(1)]);
    }

    #[test]
    fn test_rule_ref() {
        let grm = YaccGrammar::new(YaccKind::Original,
                           "%start R %token T %% R : S; S: 'T';").unwrap();

        grm.rule_idx("^").unwrap();
        grm.rule_idx("R").unwrap();
        grm.rule_idx("S").unwrap();
        grm.token_idx("T").unwrap();
        assert!(grm.token_name(grm.eof_token_idx()).is_none());

        assert_eq!(grm.rules_prods, vec![vec![PIdx(2)],
                                         vec![PIdx(0)],
                                         vec![PIdx(1)]]);
        let start_prod = grm.prod(grm.rules_prods[usize::from(grm.rule_idx("^").unwrap())][0]);
        assert_eq!(*start_prod, [Symbol::Rule(grm.rule_idx("R").unwrap())]);
        let r_prod = grm.prod(grm.rules_prods[usize::from(grm.rule_idx("R").unwrap())][0]);
        assert_eq!(r_prod.len(), 1);
        assert_eq!(r_prod[0], Symbol::Rule(grm.rule_idx("S").unwrap()));
        let s_prod = grm.prod(grm.rules_prods[usize::from(grm.rule_idx("S").unwrap())][0]);
        assert_eq!(s_prod.len(), 1);
        assert_eq!(s_prod[0], Symbol::Token(grm.token_idx("T").unwrap()));
    }

    #[test]
    fn test_long_prod() {
        let grm = YaccGrammar::new(YaccKind::Original,
                           "%start R %token T1 T2 %% R : S 'T1' S; S: 'T2';").unwrap();

        grm.rule_idx("^").unwrap();
        grm.rule_idx("R").unwrap();
        grm.rule_idx("S").unwrap();
        grm.token_idx("T1").unwrap();
        grm.token_idx("T2").unwrap();

        assert_eq!(grm.rules_prods, vec![vec![PIdx(2)],
                                         vec![PIdx(0)],
                                         vec![PIdx(1)]]);
        assert_eq!(grm.prods_rules, vec![RIdx(1),
                                         RIdx(2),
                                         RIdx(0)]);
        let start_prod = grm.prod(grm.rules_prods[usize::from(grm.rule_idx("^").unwrap())][0]);
        assert_eq!(*start_prod, [Symbol::Rule(grm.rule_idx("R").unwrap())]);
        let r_prod = grm.prod(grm.rules_prods[usize::from(grm.rule_idx("R").unwrap())][0]);
        assert_eq!(r_prod.len(), 3);
        assert_eq!(r_prod[0], Symbol::Rule(grm.rule_idx("S").unwrap()));
        assert_eq!(r_prod[1], Symbol::Token(grm.token_idx("T1").unwrap()));
        assert_eq!(r_prod[2], Symbol::Rule(grm.rule_idx("S").unwrap()));
        let s_prod = grm.prod(grm.rules_prods[usize::from(grm.rule_idx("S").unwrap())][0]);
        assert_eq!(s_prod.len(), 1);
        assert_eq!(s_prod[0], Symbol::Token(grm.token_idx("T2").unwrap()));
    }

    #[test]
    fn test_prods_rules() {
        let grm = YaccGrammar::new(YaccKind::Original, "
            %start A
            %%
            A: B
             | C;
            B: 'x';
            C: 'y'
             | 'z';
          ").unwrap();

        assert_eq!(grm.prods_rules, vec![RIdx(1),
                                         RIdx(1),
                                         RIdx(2),
                                         RIdx(3),
                                         RIdx(3),
                                         RIdx(0)]);
    }

    #[test]
    fn test_left_right_nonassoc_precs() {
        let grm = YaccGrammar::new(YaccKind::Original, "
            %start Expr
            %right '='
            %left '+' '-'
            %left '/'
            %left '*'
            %nonassoc '~'
            %%
            Expr : Expr '=' Expr
                 | Expr '+' Expr
                 | Expr '-' Expr
                 | Expr '/' Expr
                 | Expr '*' Expr
                 | Expr '~' Expr
                 | 'id' ;
          ").unwrap();

        assert_eq!(grm.prod_precs.len(), 8);
        assert_eq!(grm.prod_precs[0].unwrap(), Precedence{level: 0, kind: AssocKind::Right});
        assert_eq!(grm.prod_precs[1].unwrap(), Precedence{level: 1, kind: AssocKind::Left});
        assert_eq!(grm.prod_precs[2].unwrap(), Precedence{level: 1, kind: AssocKind::Left});
        assert_eq!(grm.prod_precs[3].unwrap(), Precedence{level: 2, kind: AssocKind::Left});
        assert_eq!(grm.prod_precs[4].unwrap(), Precedence{level: 3, kind: AssocKind::Left});
        assert_eq!(grm.prod_precs[5].unwrap(), Precedence{level: 4, kind: AssocKind::Nonassoc});
        assert!(grm.prod_precs[6].is_none());
        assert_eq!(grm.prod_precs[7], None);
    }

    #[test]
    fn test_prec_override() {
        let grm = YaccGrammar::new(YaccKind::Original, "
            %start expr
            %left '+' '-'
            %left '*' '/'
            %%
            expr : expr '+' expr
                 | expr '-' expr
                 | expr '*' expr
                 | expr '/' expr
                 | '-'  expr %prec '*'
                 | 'id' ;
        ").unwrap();
        assert_eq!(grm.prod_precs.len(), 7);
        assert_eq!(grm.prod_precs[0].unwrap(), Precedence{level: 0, kind: AssocKind::Left});
        assert_eq!(grm.prod_precs[1].unwrap(), Precedence{level: 0, kind: AssocKind::Left});
        assert_eq!(grm.prod_precs[2].unwrap(), Precedence{level: 1, kind: AssocKind::Left});
        assert_eq!(grm.prod_precs[3].unwrap(), Precedence{level: 1, kind: AssocKind::Left});
        assert_eq!(grm.prod_precs[4].unwrap(), Precedence{level: 1, kind: AssocKind::Left});
        assert!(grm.prod_precs[5].is_none());
        assert_eq!(grm.prod_precs[6], None);
    }

    #[test]
    fn test_implicit_tokens_rewrite() {
        let grm = YaccGrammar::new(YaccKind::Eco, "
          %implicit_tokens ws1 ws2
          %start S
          %%
          S: 'a' | T;
          T: 'c' |;
          ").unwrap();

        // Check that the above grammar has been rewritten to:
        //   ^ : ^~;
        //   ^~: ~ S;
        //   ~ : ws1 | ws2 | ;
        //   S : 'a' ~ | T;
        //   T : 'c' ~ | ;

        assert_eq!(grm.prod_precs.len(), 9);

        let itfs_rule_idx = grm.rule_idx(IMPLICIT_START_RULE).unwrap();
        assert_eq!(grm.rules_prods[usize::from(itfs_rule_idx)].len(), 1);

        let itfs_prod1 = &grm.prods[usize::from(grm.rules_prods[usize::from(itfs_rule_idx)][0])];
        assert_eq!(itfs_prod1.len(), 2);
        assert_eq!(itfs_prod1[0], Symbol::Rule(grm.rule_idx(IMPLICIT_RULE).unwrap()));
        assert_eq!(itfs_prod1[1], Symbol::Rule(grm.rule_idx(&"S").unwrap()));

        let s_rule_idx = grm.rule_idx(&"S").unwrap();
        assert_eq!(grm.rules_prods[usize::from(s_rule_idx)].len(), 2);

        let s_prod1 = &grm.prods[usize::from(grm.rules_prods[usize::from(s_rule_idx)][0])];
        assert_eq!(s_prod1.len(), 2);
        assert_eq!(s_prod1[0], Symbol::Token(grm.token_idx("a").unwrap()));
        assert_eq!(s_prod1[1], Symbol::Rule(grm.rule_idx(IMPLICIT_RULE).unwrap()));

        let s_prod2 = &grm.prods[usize::from(grm.rules_prods[usize::from(s_rule_idx)][1])];
        assert_eq!(s_prod2.len(), 1);
        assert_eq!(s_prod2[0], Symbol::Rule(grm.rule_idx("T").unwrap()));

        let t_rule_idx = grm.rule_idx(&"T").unwrap();
        assert_eq!(grm.rules_prods[usize::from(s_rule_idx)].len(), 2);

        let t_prod1 = &grm.prods[usize::from(grm.rules_prods[usize::from(t_rule_idx)][0])];
        assert_eq!(t_prod1.len(), 2);
        assert_eq!(t_prod1[0], Symbol::Token(grm.token_idx("c").unwrap()));
        assert_eq!(t_prod1[1], Symbol::Rule(grm.rule_idx(IMPLICIT_RULE).unwrap()));

        let t_prod2 = &grm.prods[usize::from(grm.rules_prods[usize::from(t_rule_idx)][1])];
        assert_eq!(t_prod2.len(), 0);

        assert_eq!(Some(grm.rule_idx(IMPLICIT_RULE).unwrap()), grm.implicit_rule());
        let i_rule_idx = grm.rule_idx(IMPLICIT_RULE).unwrap();
        assert_eq!(grm.rules_prods[usize::from(i_rule_idx)].len(), 3);
        let i_prod1 = &grm.prods[usize::from(grm.rules_prods[usize::from(i_rule_idx)][0])];
        let i_prod2 = &grm.prods[usize::from(grm.rules_prods[usize::from(i_rule_idx)][1])];
        assert_eq!(i_prod1.len(), 2);
        assert_eq!(i_prod2.len(), 2);
        // We don't know what order the implicit rule will contain our tokens in,
        // hence the awkward dance below.
        let cnd1 = vec![Symbol::Token(grm.token_idx("ws1").unwrap()),
                        Symbol::Rule(grm.implicit_rule().unwrap())];
        let cnd2 = vec![Symbol::Token(grm.token_idx("ws2").unwrap()),
                        Symbol::Rule(grm.implicit_rule().unwrap())];
        assert!((*i_prod1 == cnd1 && *i_prod2 == cnd2) || (*i_prod1 == cnd2 && *i_prod2 == cnd1));
        let i_prod3 = &grm.prods[usize::from(grm.rules_prods[usize::from(i_rule_idx)][2])];
        assert_eq!(i_prod3.len(), 0);
    }

    #[test]
    fn test_has_path() {
        let grm = YaccGrammar::new(YaccKind::Original, "
            %start A
            %%
            A: B;
            B: B 'x' | C;
            C: C 'y' | ;
          ").unwrap();

        let a_ridx = grm.rule_idx(&"A").unwrap();
        let b_ridx = grm.rule_idx(&"B").unwrap();
        let c_ridx = grm.rule_idx(&"C").unwrap();
        assert!(grm.has_path(a_ridx, b_ridx));
        assert!(grm.has_path(a_ridx, c_ridx));
        assert!(grm.has_path(b_ridx, b_ridx));
        assert!(grm.has_path(b_ridx, c_ridx));
        assert!(grm.has_path(c_ridx, c_ridx));
        assert!(!grm.has_path(a_ridx, a_ridx));
        assert!(!grm.has_path(b_ridx, a_ridx));
        assert!(!grm.has_path(c_ridx, a_ridx));
    }

    #[test]
    fn test_rule_min_costs() {
        let grm = YaccGrammar::new(YaccKind::Original, "
            %start A
            %%
            A: A B | ;
            B: C | D | E;
            C: 'x' B | 'x';
            D: 'y' B | 'y' 'z';
            E: 'x' A | 'x' 'y';
          ").unwrap();

        let scores = rule_min_costs(&grm, &vec![1, 1, 1]);
        assert_eq!(scores[usize::from(grm.rule_idx(&"A").unwrap())], 0);
        assert_eq!(scores[usize::from(grm.rule_idx(&"B").unwrap())], 1);
        assert_eq!(scores[usize::from(grm.rule_idx(&"C").unwrap())], 1);
        assert_eq!(scores[usize::from(grm.rule_idx(&"D").unwrap())], 2);
        assert_eq!(scores[usize::from(grm.rule_idx(&"E").unwrap())], 1);
    }

    #[test]
    fn test_min_sentences() {
        let grm = YaccGrammar::new(YaccKind::Original, "
            %start A
            %%
            A: A B | ;
            B: C | D;
            C: 'x' B | 'x';
            D: 'y' B | 'y' 'z';
          ").unwrap();

        let sg = grm.sentence_generator(|_| 1);

        let find = |nt_name: &str, str_cnds: Vec<Vec<&str>>| {
            let cnds = str_cnds.iter()
                               .map(|x| x.iter()
                                         .map(|y| grm.token_idx(y)
                                                     .unwrap())
                                         .collect::<Vec<_>>())
                               .collect::<Vec<_>>();

            let ms = sg.min_sentence(grm.rule_idx(nt_name).unwrap());
            if !cnds.iter().any(|x| x == &ms) {
                panic!("{:?} doesn't have any matches in {:?}", ms, str_cnds);
            }

            let min_sts = sg.min_sentences(grm.rule_idx(nt_name).unwrap());
            assert_eq!(cnds.len(), min_sts.len());
            for ms in min_sts {
                if !cnds.iter().any(|x| x == &ms) {
                    panic!("{:?} doesn't have any matches in {:?}", ms, str_cnds);
                }
            }
        };

        find("A", vec![vec![]]);
        find("B", vec![vec!["x"]]);
        find("C", vec![vec!["x"]]);
        find("D", vec![vec!["y", "x"], vec!["y", "z"]]);
    }

    #[test]
    fn test_rule_max_costs1() {
        let grm = YaccGrammar::new(YaccKind::Original, "
            %start A
            %%
            A: A B | ;
            B: C | D | E;
            C: 'x' B | 'x';
            D: 'y' B | 'y' 'z';
            E: 'x' A | 'x' 'y';
          ").unwrap();

        let scores = rule_max_costs(&grm, &vec![1, 1, 1]);
        assert_eq!(scores[usize::from(grm.rule_idx("A").unwrap())], u16::max_value());
        assert_eq!(scores[usize::from(grm.rule_idx("B").unwrap())], u16::max_value());
        assert_eq!(scores[usize::from(grm.rule_idx("C").unwrap())], u16::max_value());
        assert_eq!(scores[usize::from(grm.rule_idx("D").unwrap())], u16::max_value());
        assert_eq!(scores[usize::from(grm.rule_idx("E").unwrap())], u16::max_value());
    }

    #[test]
    fn test_rule_max_costs2() {
        let grm = YaccGrammar::new(YaccKind::Original, "
            %start A
            %%
            A: A B | B;
            B: C | D;
            C: 'x' 'y' | 'x';
            D: 'y' 'x' | 'y' 'x' 'z';
          ").unwrap();

        let scores = rule_max_costs(&grm, &vec![1, 1, 1]);
        assert_eq!(scores[usize::from(grm.rule_idx("A").unwrap())], u16::max_value());
        assert_eq!(scores[usize::from(grm.rule_idx("B").unwrap())], 3);
        assert_eq!(scores[usize::from(grm.rule_idx("C").unwrap())], 2);
        assert_eq!(scores[usize::from(grm.rule_idx("D").unwrap())], 3);
    }

    #[test]
    fn test_out_of_order_productions() {
        // Example taken from p54 of Locally least-cost error repair in LR parsers, Carl Cerecke
        let grm = YaccGrammar::new(YaccKind::Original, "
            %start S
            %%
            S: A 'c' 'd'
             | B 'c' 'e';
            A: 'a';
            B: 'a'
             | 'b';
            A: 'b';
            ").unwrap();

        assert_eq!(grm.prods_rules, vec![RIdx(1),
                                         RIdx(1),
                                         RIdx(2),
                                         RIdx(3),
                                         RIdx(3),
                                         RIdx(2),
                                         RIdx(0)]);
    }
}