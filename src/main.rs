use std::{env, fs, path::Path};

use impl_gc::{
    classfile::ClassLoader,
    gc::collector::Collector,
    heap::bump::BumpAllocator,
    interpreter::{ExecResult, Interpreter, value::Value},
    mutator::Mutator,
};

fn main() {
    let mut trace = false;
    let mut selectors: Vec<String> = Vec::new();

    for arg in env::args().skip(1) {
        if arg == "--trace" {
            trace = true;
        } else {
            selectors.push(arg);
        }
    }

    let mut loader = ClassLoader::default();
    let mut loaded = load_sample_classes(&mut loader, "sample");
    loaded.sort();

    if loaded.is_empty() {
        eprintln!("no .class files found under sample/");
        std::process::exit(1);
    }

    println!("impl-gc");
    println!("loaded {} classes from sample/:", loaded.len());
    for class_name in &loaded {
        if let Some(class) = loader.get(class_name) {
            println!(
                "  {:<20} methods={:<3} fields={:<3} instance_size={}",
                class_name,
                class.methods.len(),
                class.fields.len(),
                class.instance_size
            );
        }
    }

    let young_mb = env_usize("IMPL_GC_YOUNG_MB", 64);
    let old_mb = env_usize("IMPL_GC_OLD_MB", 256);
    let collector = Collector::new(young_mb * 1024 * 1024, old_mb * 1024 * 1024);

    let mutator = Mutator::new(
        BumpAllocator::from_region(collector.young_region()),
        collector.card_table(),
        collector.old_region(),
        collector.young_region(),
        &collector.safepoint,
    );

    let mut interpreter = Interpreter::new(mutator, loader);
    interpreter.set_trace(trace);

    let mut entrypoints = find_main_classes(&interpreter);
    entrypoints.sort();

    if !selectors.is_empty() {
        entrypoints.retain(|class_name| selector_matches(&selectors, class_name));
    }

    if entrypoints.is_empty() {
        eprintln!("no main([Ljava/lang/String;)V entrypoint matched");
        std::process::exit(1);
    }

    println!();
    println!("running entrypoints:");

    let mut all_ok = true;

    for class_name in &entrypoints {
        let (bytecode, max_locals, max_stack, method_name) = {
            let class = interpreter.loader.get(class_name).unwrap();
            let method = class.find_method("main", "([Ljava/lang/String;)V").unwrap();
            (
                method.bytecode.clone(),
                method.max_locals,
                method.max_stack,
                method.name,
            )
        };

        println!("  -> {}.main([Ljava/lang/String;)V", class_name);
        let result = interpreter.execute(
            class_name.clone(),
            bytecode,
            max_locals,
            max_stack,
            vec![Value::NULL],
            method_name,
        );

        all_ok &= print_exec_result("main", result);
    }

    println!();
    println!("static probes:");

    let mut probe_count = 0usize;
    for class_name in &loaded {
        let probes = collect_probe_methods(&interpreter, class_name);
        for (method_name, descriptor) in probes {
            probe_count += 1;
            let label = format!("{}.{}{}", class_name, method_name, descriptor);
            let result =
                interpreter.invoke_static(class_name, &method_name, &descriptor, Vec::new());
            all_ok &= print_exec_result(&label, result);
        }
    }

    if probe_count == 0 {
        println!("  (no probe candidates found)");
    }

    if !all_ok {
        std::process::exit(1);
    }
}

fn load_sample_classes(loader: &mut ClassLoader, sample_dir: &str) -> Vec<String> {
    let mut loaded = Vec::new();
    let entries = match fs::read_dir(sample_dir) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("failed to read {}: {}", sample_dir, error);
            return loaded;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("class") {
            continue;
        }

        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("failed to read {}: {}", path.display(), error);
                continue;
            }
        };

        match loader.load(&bytes) {
            Ok(class_name) => loaded.push(class_name),
            Err(error) => eprintln!("failed to load {}: {}", path.display(), error),
        }
    }

    loaded
}

fn find_main_classes(interpreter: &Interpreter) -> Vec<String> {
    interpreter
        .loader
        .class_names()
        .into_iter()
        .filter(|class_name| {
            interpreter
                .loader
                .get(class_name)
                .and_then(|class| class.find_method("main", "([Ljava/lang/String;)V"))
                .is_some()
        })
        .collect()
}

fn collect_probe_methods(interpreter: &Interpreter, class_name: &str) -> Vec<(String, String)> {
    let mut probes = Vec::new();
    let Some(class) = interpreter.loader.get(class_name) else {
        return probes;
    };

    for method in &class.methods {
        if !method.is_static || method.is_native {
            continue;
        }

        if method.name == "main" || method.name == "<clinit>" {
            continue;
        }

        if !method.descriptor.starts_with("()") {
            continue;
        }

        if method.descriptor.ends_with('V') {
            continue;
        }

        probes.push((method.name.to_string(), method.descriptor.to_string()));
    }

    probes
}

fn selector_matches(selectors: &[String], class_name: &str) -> bool {
    selectors.iter().any(|selector| {
        let selector = normalize_selector(selector);
        selector == class_name
            || class_name
                .rsplit('/')
                .next()
                .is_some_and(|tail| tail == selector)
    })
}

fn normalize_selector(selector: &str) -> String {
    if selector.ends_with(".class") {
        Path::new(selector)
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or(selector)
            .to_string()
    } else {
        selector.to_string()
    }
}

fn print_exec_result(label: &str, result: ExecResult) -> bool {
    match result {
        ExecResult::ReturnVoid => {
            println!("     {:<45} => ok", label);
            true
        }
        ExecResult::ReturnValue(value) => {
            println!("     {:<45} => {}", label, value_string(value));
            true
        }
        ExecResult::Exception(error) => {
            eprintln!("     {:<45} => exception\n{}", label, error);
            false
        }
        ExecResult::OutOfMemory => {
            eprintln!("     {:<45} => java.lang.OutOfMemoryError", label);
            false
        }
    }
}

fn value_string(value: Value) -> String {
    match value {
        Value::Int(v) => v.to_string(),
        Value::Long(v) => v.to_string(),
        Value::Float(v) => v.to_string(),
        Value::Double(v) => v.to_string(),
        Value::Reference(ptr) if ptr.is_null() => "null".into(),
        Value::Reference(ptr) => format!("<ref @{:p}>", ptr),
        Value::Void => "void".into(),
    }
}

fn env_usize(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}
