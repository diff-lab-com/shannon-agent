//! Manual multi-line input test
//! Run with: cargo test --package shannon-ui --test multiline_test -- --nocapture

use shannon_ui::repl_enhancement::InputBuffer;

#[test]
fn test_multiline_basic() {
    println!("\n=== Shannon Multi-Line Input Test ===\n");

    // Test 1: Basic multi-line input
    println!("Test 1: Basic multi-line input");
    let mut buf = InputBuffer::new();
    buf.insert_char('H');
    buf.insert_char('e');
    buf.insert_char('l');
    buf.insert_char('l');
    buf.insert_char('o');
    println!("  After typing 'Hello': '{}'", buf.text());
    assert_eq!(buf.text(), "Hello");

    // Test Shift+Enter (newline)
    buf.newline();
    println!(
        "  After Shift+Enter: line_count={}, text='{}'",
        buf.line_count(),
        buf.text()
    );
    assert_eq!(buf.line_count(), 2);

    buf.insert_char('W');
    buf.insert_char('o');
    buf.insert_char('r');
    buf.insert_char('l');
    buf.insert_char('d');
    println!("  After typing 'World': '{}'", buf.text());
    assert_eq!(buf.text(), "Hello\nWorld");
    println!("  ✅ PASS\n");

    // Test 2: Cursor movement in multi-line
    println!("Test 2: Vertical cursor navigation");
    let mut buf2 = InputBuffer::new();
    buf2.set_text("Line 1\nLine 2\nLine 3");
    println!("  Set text: '{}'", buf2.text());
    println!(
        "  Initial: row={}, col={}",
        buf2.cursor_row(),
        buf2.cursor_col()
    );

    // Move up from line 3 (row 2) -> line 2 (row 1)
    buf2.move_up();
    println!(
        "  After up: row={}, col={}",
        buf2.cursor_row(),
        buf2.cursor_col()
    );
    assert_eq!(buf2.cursor_row(), 1);

    // Move up to line 1 (row 0)
    buf2.move_up();
    println!(
        "  After up: row={}, col={}",
        buf2.cursor_row(),
        buf2.cursor_col()
    );
    assert_eq!(buf2.cursor_row(), 0);

    // Move down to line 2 (row 1)
    buf2.move_down();
    println!(
        "  After down: row={}, col={}",
        buf2.cursor_row(),
        buf2.cursor_col()
    );
    assert_eq!(buf2.cursor_row(), 1);

    // Move down to line 3 (row 2)
    buf2.move_down();
    println!(
        "  After down: row={}, col={}",
        buf2.cursor_row(),
        buf2.cursor_col()
    );
    assert_eq!(buf2.cursor_row(), 2);
    println!("  ✅ PASS\n");

    // Test 3: Backspace across line boundary
    println!("Test 3: Backspace at line start");
    let mut buf3 = InputBuffer::new();
    buf3.set_text("First\nSecond");
    println!("  Initial text: '{}'", buf3.text());

    // Move to start of second line by going up then left to start
    buf3.move_up(); // row 1 -> 0
    buf3.move_down(); // row 0 -> 1
    // We're now at end of line 1, need to go to start
    while buf3.cursor_col() > 0 {
        buf3.move_left();
    }

    buf3.backspace();
    println!("  After backspace at line start: '{}'", buf3.text());
    assert_eq!(buf3.text(), "FirstSecond");
    assert_eq!(buf3.line_count(), 1);
    println!("  ✅ PASS\n");

    // Test 4: Auto-indent
    println!("Test 4: Auto-indent");
    let mut buf4 = InputBuffer::new();
    buf4.set_auto_indent(true);
    buf4.insert_char(' ');
    buf4.insert_char(' ');
    buf4.insert_char('x');
    buf4.newline();
    println!(
        "  After newline from '  x': cursor_col={}",
        buf4.cursor_col()
    );
    assert_eq!(buf4.cursor_col(), 2); // Auto-indented
    buf4.insert_char('y');
    println!("  Text with indent: '{}'", buf4.text());
    assert_eq!(buf4.text(), "  x\n  y");
    println!("  ✅ PASS\n");

    // Test 5: Complex multi-line scenario
    println!("Test 5: Complex multi-line input");
    let mut buf5 = InputBuffer::new();
    buf5.insert_char('f');
    buf5.insert_char('n');
    buf5.insert_char(' ');
    buf5.insert_char('t');
    buf5.insert_char('e');
    buf5.insert_char('s');
    buf5.insert_char('t');
    buf5.newline();
    buf5.insert_char('{');
    buf5.newline();
    buf5.insert_char(' ');
    buf5.insert_char(' ');
    buf5.insert_char('r');
    buf5.insert_char('e');
    buf5.insert_char('t');
    buf5.insert_char('u');
    buf5.insert_char('r');
    buf5.insert_char('n');
    buf5.insert_char(' ');
    buf5.insert_char('t');
    buf5.insert_char('r');
    buf5.insert_char('u');
    buf5.insert_char('e');
    buf5.insert_char(';');

    let result = buf5.text();
    println!("  Result:\n{result}");
    assert!(result.contains('\n'));
    assert_eq!(buf5.line_count(), 3);
    println!("  ✅ PASS\n");

    println!("=== All Tests Passed ===");
    println!("\nSummary:");
    println!("  - Multi-line input: ✅");
    println!("  - Vertical navigation: ✅");
    println!("  - Backspace across lines: ✅");
    println!("  - Auto-indent: ✅");
    println!("  - Complex scenarios: ✅");
}
