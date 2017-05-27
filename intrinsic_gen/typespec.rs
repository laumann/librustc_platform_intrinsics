use regex::Regex;
use regex::Captures;
use std::collections::HashMap;
use std::ascii::AsciiExt;

lazy_static! {
    static ref SPEC: Regex = Regex::new(concat!(
        r#"^(?:(?P<void>V)|(?P<id>[iusfIUSF])(?:\((?P<start>\d+)-(?P<end>\d+)\)|"#,
        r#"(?P<width>\d+)(:?/(?P<llvm_width>\d+))?)"#,
        r#"|(?P<reference>\d+))(?P<index>\.\d+)?(?P<modifiers>[vShdnwusfDMC]*)(?P<force_width>x\d+)?"#,
        r#"(?:(?P<pointer>Pm|Pc)(?P<llvm_pointer>/.*)?|(?P<bitcast>->.*))?$"#
    )).unwrap();
}

lazy_static! {
    static ref TYPE_ID_LOOKUP: HashMap<char, &'static [NumKind]> = {
        static SU: &'static [NumKind] = &[NumKind::Signed, NumKind::Unsigned];
        static S:  &'static [NumKind] = &[NumKind::Signed];
        static U:  &'static [NumKind] = &[NumKind::Unsigned];
        static F:  &'static [NumKind] = &[NumKind::Float];
        let mut hm = HashMap::new();
        hm.insert('i', SU);
        hm.insert('s', S);
        hm.insert('u', U);
        hm.insert('f', F);
        hm
    };
}
pub struct TypeSpec {
    spec: Vec<String>,
}

impl TypeSpec {
    fn from_str(s: &str) -> TypeSpec {
        let v = vec![s.to_string()];
        TypeSpec { spec: v }
    }

    fn from_list(v: Vec<String>) -> TypeSpec {
        TypeSpec { spec: v }
    }

    fn enumerate(&self, width: i32, previous: &[Type]) -> Vec<Type> {

        let mut result = vec![];
        for spec in &self.spec {
            let caps = SPEC.captures(&spec);
            if let Some(caps) = caps {
                let id = caps.name("id");
                let reference = caps.name("reference");

                let mut modifiers: Vec<String> = vec![];
                if let Some(index) = caps.name("index") {
                    modifiers.push(index.as_str().to_string());
                }
                if let Some(m) = caps.name("modifiers") {
                    for c in m.as_str().chars() {
                        modifiers.push(c.to_string());
                    }
                }
                if let Some(fw) = caps.name("force_width") {
                    modifiers.push(fw.as_str().to_string());
                }
                if let Some(bc) = caps.name("bitcast") {
                    modifiers.push(bc.as_str().to_string());
                }

                if let Some(v) = caps.name("void") {
                    result.push(Type::Void);
                } else if let Some(id) = id {
                    let id = id.as_str().chars().next().unwrap();
                    let is_vector = id.is_ascii_lowercase();
                    let type_ctors =
                        TYPE_ID_LOOKUP
                            .get(&id.to_ascii_lowercase())
                            .expect(&format!("find TYPE_ID {} failed", id.to_ascii_lowercase()));

                    let (start, end, llvm_width);
                    if let Some(s) = caps.name("start") {
                        start = s.as_str();
                        end = &caps["end"];
                        llvm_width = None;
                    } else {
                        start = &caps["width"];
                        end = &caps["width"];
                        llvm_width = caps.name("llvm_width");
                    }
                    let start: i32 = start.parse().unwrap();
                    let end: i32 = end.parse().unwrap();
                    let mut bitwidth = start;
                    while bitwidth <= end {
                        for ctor in *type_ctors {
                            let scalar = if let Some(llvm_width) = llvm_width {
                                assert!(!is_vector);
                                let llvm_width: i32 = llvm_width.as_str().parse().unwrap();
                                assert!(llvm_width < bitwidth);
                                Number {
                                    kind: *ctor,
                                    bitwidth: bitwidth,
                                    llvm_bitwidth: Some(llvm_width),
                                }
                            } else {
                                Number {
                                    kind: *ctor,
                                    bitwidth: bitwidth,
                                    llvm_bitwidth: None,
                                }
                            };
                            let mut elem = if is_vector {
                                Type::Vector {
                                    elem: Box::new(Type::Num(scalar)),
                                    length: width / bitwidth,
                                    bitcast: None,
                                }
                            } else {
                                Type::Num(scalar)
                            };

                            for x in &modifiers {
                                elem = elem.modify(&x, width, previous);
                            }
                            result.push(ptrify(&caps, elem, width, previous));
                        }
                        bitwidth *= 2;
                    }
                } else if let Some(reference) = reference {
                    let reference: usize = reference.as_str().parse().unwrap();
                    assert!(reference < previous.len(),
                            format!("referring to argument {}, but only {} are known",
                                    reference,
                                    previous.len()));
                    let mut ret = previous[reference].clone();
                    for x in modifiers {
                        ret = ret.modify(&x, width, previous);
                    }
                    result.push(ptrify(&caps, ret, width, &previous));
                } else {
                    assert!(false,
                            format!("matched `{}`, but didn\'t understand it?", spec))
                }
            } else if spec.starts_with('(') {
                let len = spec.len();
                let (true_spec, flatten) = if spec.ends_with(')') {
                    (&spec[1..len - 2], false)
                } else if spec.ends_with(")f") {
                    (&spec[1..len - 3], true)
                } else {
                    panic!("found unclosed aggregate {}", spec)
                };
                // TODO
            } else if spec.starts_with('[') {
                // TODO
            } else {
                panic!("Failed to parse {}", spec);
            }
        }
        result
    }
}

#[derive(Clone)]
pub enum Type {
    Void,
    Num(Number),
    Pointer {
        elem: Box<Type>,
        llvm_elem: Option<Box<Type>>,
        is_const: bool,
    },
    Vector {
        elem: Box<Type>,
        length: i32,
        bitcast: Option<Box<Type>>,
    },
    Aggregate { flatten: bool, elems: Vec<Type> },
}

#[derive(Copy, Clone)]
enum NumKind {
    Signed,
    Unsigned,
    Float,
}

#[derive(Copy, Clone)]
pub struct Number {
    kind: NumKind,
    bitwidth: i32,
    llvm_bitwidth: Option<i32>,
}

struct PlatformTypeInfo {}

impl Type {
    pub fn bitwidth(&self) -> i32 {
        match self {
            &Type::Void => 0,
            &Type::Num(ref n) => n.bitwidth,
            _ => 0,
        }
    }

    pub fn compiler_ctor(&self) -> String {
        match self {
            &Type::Void => "::VOID".to_string(),
            &Type::Num(ref n) => n.compiler_ctor(),
            &Type::Pointer {
                elem: ref e,
                llvm_elem: ref le,
                is_const: c,
            } => {
                let llvm_elem = if let &Some(ref le) = le {
                    format!("Some({})", le.compiler_ctor_ref())
                } else {
                    "None".to_string()
                };
                format!("Type::Pointer({}, {}, {})",
                        e.compiler_ctor_ref(),
                        llvm_elem,
                        c)
            }
            &Type::Vector {
                elem: ref e,
                length: l,
                bitcast: ref bc,
            } => {
                if let &Some(ref bc) = bc {
                    format!("{}x{}_{}",
                            e.compiler_ctor(),
                            l,
                            bc.compiler_ctor().replace("::", ""))
                } else {
                    format!("{}x{}", e.compiler_ctor(), l)
                }
            }
            &Type::Aggregate {
                flatten: f,
                elems: ref e,
            } => {
                let parts = format!("{{ static PARTS: [&'static Type; {}] = [{}]; &PARTS }}",
                                    e.len(),
                                    e.iter()
                                        .map(|ref x| x.compiler_ctor_ref())
                                        .collect::<Vec<String>>()
                                        .join(", "));
                format!("Type::Aggregate({}, {})", f, parts)
            }
        }
    }

    pub fn compiler_ctor_ref(&self) -> String {
        let mut cc = self.compiler_ctor();
        match self {
            &Type::Pointer { .. } => format!("{{ static PTR: Type = {}; &PTR }}", cc),
            &Type::Aggregate { .. } => format!("{{ static AGG: Type = {}; &AGG }}", cc),
            _ => {
                cc.insert(0, '&');
                cc
            }
        }
    }

    pub fn rust_name(&self) -> String {
        match self {
            &Type::Void => "()".to_string(),
            &Type::Num(ref n) => n.rust_name(),
            &Type::Pointer {
                elem: ref e,
                llvm_elem: ref le,
                is_const: c,
            } => {
                let modifier = if c { "const" } else { "mut" };
                format!("*{} {}", modifier, e.rust_name())
            }
            &Type::Vector {
                elem: ref e,
                length: l,
                bitcast: ref bc,
            } => format!("{}x{}", e.rust_name(), l),
            &Type::Aggregate {
                flatten: f,
                elems: ref e,
            } => {
                format!("({})",
                        e.iter()
                            .map(|ref x| x.rust_name())
                            .collect::<Vec<String>>()
                            .join(", "))
            }
        }
    }

    pub fn modify(self, spec: &str, width: i32, previous: &[Type]) -> Type {
        match self {
            Type::Void => self,
            Type::Num(ref n) => {
                match spec {
                    "u" => {
                        Type::Num(Number {
                                      kind: NumKind::Unsigned,
                                      bitwidth: n.bitwidth,
                                      llvm_bitwidth: None,
                                  })
                    }
                    "s" => {
                        Type::Num(Number {
                                      kind: NumKind::Signed,
                                      bitwidth: n.bitwidth,
                                      llvm_bitwidth: None,
                                  })
                    }
                    "f" => {
                        Type::Num(Number {
                                      kind: NumKind::Float,
                                      bitwidth: n.bitwidth,
                                      llvm_bitwidth: None,
                                  })
                    }
                    "w" => {
                        Type::Num(Number {
                                      kind: n.kind,
                                      bitwidth: n.bitwidth * 2,
                                      llvm_bitwidth: None,
                                  })
                    }
                    "n" => {
                        Type::Num(Number {
                                      kind: n.kind,
                                      bitwidth: n.bitwidth / 2,
                                      llvm_bitwidth: None,
                                  })
                    }
                    "v" => {
                        Type::Vector {
                            elem: Box::new(self.clone()),
                            length: width / n.bitwidth,
                            bitcast: None,
                        }
                    }
                    _ => panic!("unknown modification spec {}", spec),
                }
            }
            Type::Pointer {
                elem: e,
                llvm_elem: le,
                is_const: c,
            } => {
                match spec {
                    "D" => *e,
                    "M" => {
                        Type::Pointer {
                            elem: e,
                            llvm_elem: le,
                            is_const: false,
                        }
                    }
                    "C" => {
                        Type::Pointer {
                            elem: e,
                            llvm_elem: le,
                            is_const: true,
                        }
                    }
                    _ => {
                        Type::Pointer {
                            elem: Box::new(e.modify(spec, width, previous)),
                            llvm_elem: le,
                            is_const: c,
                        }
                    }
                }
            }
            Type::Vector {
                elem: e,
                length: l,
                bitcast: bc,
            } => {
                if spec == "S" {
                    *e
                } else if spec == "h" {
                    Type::Vector {
                        elem: e,
                        length: l / 2,
                        bitcast: None,
                    }
                } else if spec == "d" {
                    Type::Vector {
                        elem: e,
                        length: l * 2,
                        bitcast: None,
                    }
                } else if spec.starts_with('x') {
                    let new_bitwidth: i32 =
                        spec[1..]
                            .parse()
                            .expect("spec starts with 'x', but no integer followed");
                    let bw = e.bitwidth();
                    Type::Vector {
                        elem: e,
                        length: new_bitwidth / bw,
                        bitcast: None,
                    }
                } else if spec.starts_with("->") {
                    let bitcast_to = TypeSpec::from_str(&spec[2..]);
                    unimplemented!()
                } else {
                    unimplemented!()
                }
            }
            Type::Aggregate {
                flatten: f,
                elems: e,
            } => {
                if spec.starts_with('.') {
                    let num: usize = spec[1..]
                        .parse()
                        .expect("spec starts with '.', but no integer followed");
                    e[num].clone()
                } else {
                    unimplemented!()
                }
            }
        }
    }
}

impl Number {
    pub fn compiler_ctor(&self) -> String {
        match self.kind {
            NumKind::Signed => {
                if let Some(lw) = self.llvm_bitwidth {
                    format!("::I{}_{}", self.bitwidth, lw)
                } else {
                    format!("::I{}", self.bitwidth)
                }
            }
            NumKind::Unsigned => {
                if let Some(lw) = self.llvm_bitwidth {
                    format!("::U{}_{}", self.bitwidth, lw)
                } else {
                    format!("::U{}", self.bitwidth)
                }
            }
            NumKind::Float => format!("::F{}", self.bitwidth),
        }
    }

    pub fn rust_name(&self) -> String {
        let m = match self.kind {
            NumKind::Signed => 'i',
            NumKind::Unsigned => 'u',
            NumKind::Float => 'f',
        };
        format!("{}{}", m, self.bitwidth)
    }
}

fn ptrify(caps: &Captures, elem: Type, width: i32, previous: &[Type]) -> Type {
    unimplemented!()
}
