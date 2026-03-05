import 'package:flutter_test/flutter_test.dart';
import 'package:blaz/src/api.dart';

void main() {
  group('parseIngredientLine', () {
    // --- Fully parsed cases --------------------------------------------------

    test('quantity + unit + name', () {
      final i = parseIngredientLine('150 g flour');
      expect(i.quantity, 150.0);
      expect(i.unit, 'g');
      expect(i.name, 'flour');
      expect(i.prep, isNull);
    });

    test('quantity + unit + "of" + name', () {
      final i = parseIngredientLine('2 tsp of salt');
      expect(i.quantity, 2.0);
      expect(i.unit, 'tsp');
      expect(i.name, 'salt');
    });

    test('quantity without unit', () {
      final i = parseIngredientLine('3 eggs');
      expect(i.quantity, 3.0);
      expect(i.unit, isNull);
      expect(i.name, 'eggs');
    });

    test('decimal quantity with comma', () {
      final i = parseIngredientLine('1,5 kg potatoes');
      expect(i.quantity, 1.5);
      expect(i.unit, 'kg');
      expect(i.name, 'potatoes');
    });

    test('decimal quantity with dot', () {
      final i = parseIngredientLine('0.5 L milk');
      expect(i.quantity, 0.5);
      expect(i.unit, 'L');
      expect(i.name, 'milk');
    });

    test('multi-word name', () {
      final i = parseIngredientLine('2 tbsp olive oil');
      expect(i.quantity, 2.0);
      expect(i.unit, 'tbsp');
      expect(i.name, 'olive oil');
    });

    // --- Fallback cases (raw text) -------------------------------------------

    test('no quantity → full text becomes name', () {
      final i = parseIngredientLine('a pinch of salt');
      expect(i.quantity, isNull);
      expect(i.unit, isNull);
      expect(i.name, 'a pinch of salt');
    });

    test('empty string → empty name', () {
      final i = parseIngredientLine('');
      expect(i.quantity, isNull);
      expect(i.name, '');
    });

    test('only quantity token (no name) → raw fallback', () {
      // Only one token → can't extract name, treated as raw
      final i = parseIngredientLine('200');
      expect(i.quantity, isNull);
      expect(i.name, '200');
    });

    test('unknown unit treated as start of name', () {
      final i = parseIngredientLine('2 cups flour');
      expect(i.quantity, 2.0);
      expect(i.unit, isNull);
      expect(i.name, 'cups flour');
    });

    // --- toLine round-trip ---------------------------------------------------

    test('toLine round-trips qty + unit + name', () {
      final i = parseIngredientLine('150 g flour');
      expect(i.toLine(), '150 g flour');
    });

    test('toLine with scale', () {
      final i = parseIngredientLine('100 g butter');
      expect(i.toLine(factor: 2.0), '200 g butter');
    });

    test('toLine qty without unit', () {
      final i = parseIngredientLine('3 eggs');
      expect(i.toLine(), '3 eggs');
    });
  });

  group('Ingredient.toLine', () {
    test('shows prep when present', () {
      final i = Ingredient(quantity: 2.0, unit: null, name: 'carrots', prep: 'diced');
      expect(i.toLine(), '2 carrots, diced');
    });

    test('hides prep when includePrep=false', () {
      final i = Ingredient(quantity: 2.0, unit: null, name: 'carrots', prep: 'diced');
      expect(i.toLine(includePrep: false), '2 carrots');
    });

    test('no quantity → only name', () {
      final i = Ingredient(name: 'salt');
      expect(i.toLine(), 'salt');
    });

    test('g and ml qty are rounded to int', () {
      final i = Ingredient(quantity: 150.0, unit: 'g', name: 'flour');
      expect(i.toLine(), '150 g flour');
    });

    test('kg qty shows two decimals (trimmed)', () {
      final i = Ingredient(quantity: 1.5, unit: 'kg', name: 'potatoes');
      expect(i.toLine(), '1.5 kg potatoes');
    });
  });
}
