#[test]
fn substitution() {
    let mut context = crate::Context::new();
    context
        .macros
        .insert(String::from("Foo"), String::from("Bar"));

    assert_eq!(crate::process_str("Foo", &mut context).unwrap(), "Bar\n");
    assert_eq!(
        crate::process_str("AFooB", &mut context).unwrap(),
        "AFooB\n"
    );
    assert_eq!(crate::process_str("Foo_", &mut context).unwrap(), "Foo_\n");
    assert_eq!(crate::process_str("_Foo", &mut context).unwrap(), "_Foo\n");
    assert_eq!(
        crate::process_str("One Foo Two", &mut context).unwrap(),
        "One Bar Two\n"
    );
}

#[test]
fn define() {
    assert_eq!(
        crate::process_str("#define Baz Quux\nBaz\n", &mut crate::Context::new()).unwrap(),
        "Quux\n"
    );
    assert_eq!(
        crate::process_str(
            " # define Baz\nBaz\n#undef Quux\n # undef Baz\nBaz\n",
            &mut crate::Context::new()
        )
        .unwrap(),
        "1\nBaz\n"
    );
}

#[test]
fn context() {
    let mut context = crate::Context::new();
    context
        .macros
        .insert(String::from("$Foo"), String::from("1"));
    assert_eq!(crate::process_str("$Foo", &mut context).unwrap(), "1\n");
    assert_eq!(
        crate::process_str("#define $Foo 2", &mut context).unwrap(),
        ""
    );
    assert_eq!(crate::process_str("$Foo", &mut context).unwrap(), "2\n");
}

#[test]
fn ifdef() {
    let mut context = crate::Context::new();

    assert_eq!(
        crate::process_str(
            "#define Foo
#ifdef Foo
Bar
#endif",
            &mut context
        )
        .unwrap(),
        "Bar\n"
    );
}

#[test]
fn elif() {
    assert_eq!(
        crate::process_str(
            "#define Foo
#define Bar
#ifdef Foo
Just Foo
# ifdef Baz
No Line
# elifdef Bar
Foo and Bar
# endif
#endif",
            &mut crate::Context::new()
        )
        .unwrap(),
        "Just 1\n1 and 1\n"
    );
}

#[test]
fn ifndef() {
    assert_eq!(
        crate::process_str(
            "#define A
#ifndef A
No Text
#elifndef B
text
#endif",
            &mut crate::Context::new()
        )
        .unwrap(),
        "text\n"
    );
}

#[test]
fn include() {
    assert_eq!(
        crate::process_str(
            "#define A some_text
#include test.txt",
            &mut crate::Context::new()
        )
        .unwrap(),
        "a macro is some_text\n"
    );

    assert_eq!(
        crate::process_str(
            "#define B more_text
#include test.txt",
            &mut crate::Context::new()
        )
        .unwrap(),
        "b macro is more_text\n"
    );

    assert_eq!(
        crate::process_str("#include test.txt", &mut crate::Context::new()).unwrap(),
        "no macro\n"
    );
}

#[test]
fn include_dir() {
    assert_eq!(
        crate::process_str(
            "#include tests/include.txt",
            &mut crate::Context::new()
        )
        .unwrap(),
        "some text\n"
    );
}
