use serde_json::{self, Value};
use std::path::Path;
use std::default::Default;
use std::io::prelude::*;
use std::fs::File;
use std::fmt::{Display, Formatter, Error};
use std::slice::SliceConcatExt;
use std::string::ToString;

use typespec::Type;
use typespec::TypeSpec;

pub fn parse(p: &Path) -> Platform {

    if p.is_dir() {
        parse_dir(p)
    } else {
        parse_file(p)
    }
}

fn parse_dir(path: &Path) -> Platform {
    let mut result = Platform::default();
    let file_stem = path.file_stem()
        .map(|n| n.to_owned().into_string())
        .unwrap()
        .unwrap();
    println!("Parse dir {:?} , dir name {:?}", path, file_stem);
    result.file_stem = file_stem;

    if path.is_dir() {
        let files = path.read_dir()
            .expect(&format!("read_dir {:?} failed", path));
        for entry in files {
            if let Ok(entry) = entry {
                result.merge(parse_file(&entry.path()));
            }
        }
    } else {
        result.merge(parse_file(path));
    }
    result
}

fn parse_file(path: &Path) -> Platform {
    let mut f = File::open(path).expect(&format!("open file {:?} failed", path));
    let mut buffer = String::new();

    let file_stem = path.file_stem()
        .map(|n| n.to_owned().into_string())
        .unwrap()
        .unwrap();
    println!("Parse file {:?} , file name {:?}", path, file_stem);

    f.read_to_string(&mut buffer)
        .expect(&format!("read file {:?} failed", path));
    let json: Value = serde_json::from_str(&buffer)
        .expect(&format!("parse json failed in file {:?}", path));

    let mut p = Platform::from_json(&json);
    p.file_stem = file_stem;
    p
}

#[derive(Default, Debug)]
pub struct Platform {
    pub file_stem: String,
    platform: Option<PlatformInfo>,
    intrinsicset: Vec<IntrinsicSet>,
}

impl Platform {
    pub fn from_json(json: &Value) -> Self {
        Platform {
            file_stem: String::new(),
            platform: PlatformInfo::from_json(json),
            intrinsicset: vec![IntrinsicSet::from_json(json)],
        }
    }

    pub fn merge(&mut self, mut other: Platform) {
        if other.platform.is_some() {
            self.platform = other.platform;
        }
        self.intrinsicset.append(&mut other.intrinsicset);
    }

    pub fn platform_prefix(&self) -> String {
        if let Some(ref p) = self.platform {
            p.name.clone()
        } else {
            String::new()
        }
    }

    fn widths(&self) -> Vec<i32> {
        if let Some(ref p) = self.platform {
            p.width_info.iter().map(|w| w.width).collect()
        } else {
            vec![]
        }
    }

    pub fn monomorphise(&self) -> Vec<MonomorphicIntrinsic> {
        let mut result = vec![];
        for s in &self.intrinsicset {
            for i in &s.intrinsics {
                let ret = TypeSpec::from_list(&i.ret[..]);
                let mut args : Vec<_> = i.args.iter().map(|s|TypeSpec::from_str(s)).collect();
                for w in self.widths() {
                    assert!(w & (w - 1) == 0);
                    let p = [];
                    let mut u = vec![ret.clone()];
                    u.append(&mut args);
                    let mut r = recur(w, &p, &u[..]);
                    for mut m in &mut r {
                        m.update(w, self, s, i);
                    }
                    result.append(&mut r);
                }
            }
        }
        return result;

        fn recur(width: i32, processed: &[Type], untouched: &[TypeSpec])
            -> Vec<MonomorphicIntrinsic>
        {
            if untouched.is_empty() {
                let ret = &processed[0];
                let args = &processed[1..];
                let m = MonomorphicIntrinsic::from_types(ret, args);
                return vec![m];
            } else {
                let mut result = vec![];
                let raw_arg = &untouched[0];
                let rest = &untouched[1..];
                for arg in raw_arg.enumerate(width, processed) {
                    let mut extend : Vec<Type> = processed.into();
                    extend.push(arg);
                    for intr in recur(width, &extend[..], rest) {
                        result.push(intr);
                    }
                }
                return result;
            }
        }
    }

    pub fn generate(&self) -> String {
        self.monomorphise().iter().map(|m| m.to_string()).collect::<Vec<String>>().join("")
    }
}

#[derive(Default, Debug, Clone)]
pub struct PlatformInfo {
    name: String,
    number_info: Vec<NumberInfo>,
    width_info: Vec<WidthInfo>,
}

impl PlatformInfo {
    pub fn from_json(json: &Value) -> Option<Self> {
        let p = json.get("platform");
        let n = json.get("number_info");
        let w = json.get("width_info");
        if let Some(p) = p {
            Some(PlatformInfo {
                     name: p.to_string(),
                     number_info: if let Some(n) = n {
                         NumberInfo::from_json(n)
                     } else {
                         vec![]
                     },
                     width_info: if let Some(w) = w {
                         WidthInfo::from_json(w)
                     } else {
                         vec![]
                     },
                 })
        } else {
            None
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct NumberInfo {
    ty: String,
    props: Value,
}

impl NumberInfo {
    pub fn from_json(json: &Value) -> Vec<NumberInfo> {
        let mut res = Vec::new();
        if let &Value::Object(ref map) = json {
            for (k, v) in map {
                let item = NumberInfo {
                    ty: k.clone(),
                    props: v.clone(),
                };
                res.push(item);
            }
        }
        return res;
    }
}

#[derive(Default, Debug, Clone)]
pub struct WidthInfo {
    width: i32,
    pub props: Value,
}

impl WidthInfo {
    pub fn from_json(json: &Value) -> Vec<WidthInfo> {
        let mut res = Vec::new();
        if let &Value::Object(ref map) = json {
            for (k, v) in map {
                let item = WidthInfo {
                    width: k.parse().expect(""),
                    props: v.clone(),
                };
                res.push(item);
            }
        }
        return res;
    }
}

#[derive(Default, Debug, Clone)]
pub struct IntrinsicSet {
    intrinsic_prefix: String,
    llvm_prefix: String,
    intrinsics: Vec<IntrinsicData>,
}

impl IntrinsicSet {
    pub fn from_json(json: &Value) -> IntrinsicSet {
        let mut data = IntrinsicSet::default();
        data.intrinsic_prefix = json.get("intrinsic_prefix")
            .map(|s| s.to_string())
            .unwrap_or(String::new());
        data.llvm_prefix = json.get("llvm_prefix")
            .map(|s| s.to_string())
            .unwrap_or(String::new());

        let intrisics = json.get("intrinsics");
        if let Some(&Value::Array(ref arr)) = intrisics {
            for item in arr {
                let i = IntrinsicData::from_json(item);
                data.intrinsics.push(i);
            }
        }

        return data;
    }
}

#[derive(Default, Debug, Clone)]
pub struct IntrinsicData {
    intrinsic: String,
    width: Vec<String>,
    llvm: String,
    ret: Vec<String>,
    args: Vec<String>,
}

impl IntrinsicData {
    pub fn from_json(json: &Value) -> IntrinsicData {
        IntrinsicData {
            intrinsic: json.get("intrinsic")
                .map(|s| s.to_string())
                .unwrap_or(String::new()),
            width: read_array(json.get("width")),
            llvm: json.get("llvm")
                .map(|s| s.to_string())
                .unwrap_or(String::new()),
            ret: read_array(json.get("ret")),
            args: read_array(json.get("args")),
        }
    }
}

fn read_array(json: Option<&Value>) -> Vec<String> {
    match json {
        Some(&Value::Array(ref arr)) => arr.iter().map(|v| v.to_string().trim_matches('"').to_string()).collect(),
        Some(&Value::String(ref s)) => vec![s.to_string().trim_matches('"').to_string()],
        _ => Vec::new(),
    }
}

#[derive(Debug, Clone)]
pub struct MonomorphicIntrinsic {
    intrinsic_set_name: String,
    platform_prefix: String,
    len: usize,
    llvm_name: String,
    args: Vec<Type>,
    ret: Type,
}

impl MonomorphicIntrinsic {
    fn from_types(ret: &Type, args: &[Type]) -> Self {
        MonomorphicIntrinsic {
            intrinsic_set_name: Default::default(),
            platform_prefix: Default::default(),
            len: 0,
            llvm_name: Default::default(),
            ret: ret.clone(),
            args: args.into(),
        }
    }

    fn update(&mut self, w: i32, p: &Platform, s: &IntrinsicSet, i: &IntrinsicData) {
        self.intrinsic_set_name = s.intrinsic_prefix.clone()
                                + &i.intrinsic; // TODO: format
        self.platform_prefix = p.platform_prefix();
        self.len = self.args.len();
        self.llvm_name = if i.llvm.starts_with('!') {
            i.llvm[1..].into() // TODO: format
        } else {
            s.llvm_prefix.clone() + &i.llvm[1..] // TODO: format
        };
    }

    fn compiler_args(&self) -> String {
        self.args.iter().map(|a|a.compiler_ctor_ref()).collect::<Vec<String>>().join(",")
    }

    fn compiler_ret(&self) -> String {
        self.ret.compiler_ctor_ref()
    }
}

impl Display for MonomorphicIntrinsic {
    fn fmt(&self, f: &mut Formatter) -> Result<(), Error> {
        write!(f,
               r#"
        "{}" => Intrinsic {{
            inputs: {{ static INPUTS: [&'static Type; {}] = [{}]; &INPUTS }},
            output: &{},
            definition: Named("{}")
        }}"#,
               self.intrinsic_set_name,
               self.len,
               self.compiler_args(),
               self.compiler_ret(),
               self.llvm_name)

    }
}

pub struct TypeVec(char, i32, i32);

impl Display for TypeVec {
    fn fmt(&self, f: &mut Formatter) -> Result<(), Error> {
        write!(f, "::{}{}x{}", self.0, self.1, self.2)
    }
}
