import 'package:flutter_test/flutter_test.dart';
import 'package:blaz/src/api.dart';

void main() {
  // ──────────────────────────────────────────────────────────────────
  // Ingredient.fromJson
  // ──────────────────────────────────────────────────────────────────
  group('Ingredient.fromJson', () {
    test('parses quantity, unit and name', () {
      final ing = Ingredient.fromJson({
        'quantity': 200.0,
        'unit': 'g',
        'name': 'flour',
      });
      expect(ing.quantity, 200.0);
      expect(ing.unit, 'g');
      expect(ing.name, 'flour');
      expect(ing.prep, isNull);
    });

    test('null quantity and unit are preserved as null', () {
      final ing = Ingredient.fromJson({'name': 'salt', 'quantity': null, 'unit': null});
      expect(ing.quantity, isNull);
      expect(ing.unit, isNull);
    });

    test('empty unit string is normalised to null', () {
      final ing = Ingredient.fromJson({'name': 'pepper', 'unit': ''});
      expect(ing.unit, isNull);
    });

    test('prep field is read from "prep" key', () {
      final ing = Ingredient.fromJson({
        'name': 'garlic',
        'quantity': 3.0,
        'prep': 'minced',
      });
      expect(ing.prep, 'minced');
    });

    test('empty prep string is stored as null', () {
      final ing = Ingredient.fromJson({'name': 'onion', 'prep': '   '});
      expect(ing.prep, isNull);
    });

    test('prep_words list is joined into a single string', () {
      final ing = Ingredient.fromJson({
        'name': 'tomato',
        'prep_words': ['finely', 'chopped'],
      });
      expect(ing.prep, 'finely chopped');
    });

    test('prep takes priority over prep_words', () {
      final ing = Ingredient.fromJson({
        'name': 'onion',
        'prep': 'diced',
        'prep_words': ['sliced'],
      });
      expect(ing.prep, 'diced');
    });

    test('integer quantity is coerced to double', () {
      final ing = Ingredient.fromJson({'name': 'eggs', 'quantity': 2});
      expect(ing.quantity, 2.0);
      expect(ing.quantity, isA<double>());
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // Ingredient.toLine (IngredientFormat extension)
  // ──────────────────────────────────────────────────────────────────
  group('Ingredient.toLine', () {
    test('name only', () {
      final ing = Ingredient(name: 'salt');
      expect(ing.toLine(), 'salt');
    });

    test('quantity without unit', () {
      final ing = Ingredient(quantity: 3.0, name: 'eggs');
      expect(ing.toLine(), '3 eggs');
    });

    test('quantity with unit', () {
      final ing = Ingredient(quantity: 200.0, unit: 'g', name: 'flour');
      expect(ing.toLine(), '200 g flour');
    });

    test('g and ml are rounded to integer', () {
      final ingG = Ingredient(quantity: 250.5, unit: 'g', name: 'sugar');
      expect(ingG.toLine(), '251 g sugar');
      final ingMl = Ingredient(quantity: 120.7, unit: 'ml', name: 'cream');
      expect(ingMl.toLine(), '121 ml cream');
    });

    test('kg uses two decimal places with trailing zeros trimmed', () {
      final ing = Ingredient(quantity: 1.5, unit: 'kg', name: 'potatoes');
      expect(ing.toLine(), '1.5 kg potatoes');
    });

    test('trailing .0 is stripped for non-metric', () {
      final ing = Ingredient(quantity: 2.0, unit: 'tbsp', name: 'olive oil');
      expect(ing.toLine(), '2 tbsp olive oil');
    });

    test('prep is appended after comma', () {
      final ing = Ingredient(quantity: 2.0, name: 'garlic', prep: 'minced');
      expect(ing.toLine(), '2 garlic, minced');
    });

    test('prep is excluded when includePrep is false', () {
      final ing = Ingredient(quantity: 2.0, name: 'garlic', prep: 'minced');
      expect(ing.toLine(includePrep: false), '2 garlic');
    });

    test('factor scales the quantity', () {
      final ing = Ingredient(quantity: 100.0, unit: 'g', name: 'butter');
      expect(ing.toLine(factor: 2.0), '200 g butter');
    });

    test('factor does not apply when quantity is null', () {
      final ing = Ingredient(name: 'salt');
      expect(ing.toLine(factor: 3.0), 'salt');
    });

    test('fractional quantities are trimmed cleanly', () {
      final ing = Ingredient(quantity: 1.5, name: 'avocados');
      expect(ing.toLine(), '1.5 avocados');
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // Ingredient.toJson
  // ──────────────────────────────────────────────────────────────────
  group('Ingredient.toJson', () {
    test('round-trips quantity, unit, name', () {
      final ing = Ingredient(quantity: 100.0, unit: 'g', name: 'flour');
      final j = ing.toJson();
      expect(j['quantity'], 100.0);
      expect(j['unit'], 'g');
      expect(j['name'], 'flour');
    });

    test('prep key only present when non-null', () {
      final withPrep = Ingredient(name: 'garlic', prep: 'minced');
      expect(withPrep.toJson().containsKey('prep'), isTrue);

      final noPrep = Ingredient(name: 'salt');
      expect(noPrep.toJson().containsKey('prep'), isFalse);
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // parseIngredientLine
  // ──────────────────────────────────────────────────────────────────
  group('parseIngredientLine', () {
    test('parses quantity + canonical unit + name', () {
      final ing = parseIngredientLine('200 g flour');
      expect(ing.quantity, 200.0);
      expect(ing.unit, 'g');
      expect(ing.name, 'flour');
    });

    test('parses kg unit', () {
      final ing = parseIngredientLine('1.5 kg beef');
      expect(ing.quantity, 1.5);
      expect(ing.unit, 'kg');
      expect(ing.name, 'beef');
    });

    test('parses ml unit', () {
      final ing = parseIngredientLine('100 ml milk');
      expect(ing.quantity, 100.0);
      expect(ing.unit, 'ml');
      expect(ing.name, 'milk');
    });

    test('parses L unit', () {
      final ing = parseIngredientLine('1 L water');
      expect(ing.quantity, 1.0);
      expect(ing.unit, 'L');
      expect(ing.name, 'water');
    });

    test('parses tsp unit', () {
      final ing = parseIngredientLine('1 tsp salt');
      expect(ing.quantity, 1.0);
      expect(ing.unit, 'tsp');
      expect(ing.name, 'salt');
    });

    test('parses tbsp unit', () {
      final ing = parseIngredientLine('2 tbsp olive oil');
      expect(ing.quantity, 2.0);
      expect(ing.unit, 'tbsp');
      expect(ing.name, 'olive oil');
    });

    test('quantity without unit', () {
      final ing = parseIngredientLine('2 eggs');
      expect(ing.quantity, 2.0);
      expect(ing.unit, isNull);
      expect(ing.name, 'eggs');
    });

    test('unrecognised unit stays in name', () {
      // "cup" is not a canonical backend unit
      final ing = parseIngredientLine('1 cup sugar');
      expect(ing.quantity, 1.0);
      expect(ing.unit, isNull);
      expect(ing.name, 'cup sugar');
    });

    test('plain name only (no leading number)', () {
      final ing = parseIngredientLine('flour');
      expect(ing.quantity, isNull);
      expect(ing.unit, isNull);
      expect(ing.name, 'flour');
    });

    test('comma decimal separator is handled', () {
      final ing = parseIngredientLine('1,5 kg beef');
      expect(ing.quantity, 1.5);
      expect(ing.unit, 'kg');
    });

    test('extra words after name are preserved', () {
      final ing = parseIngredientLine('200 g flour sifted');
      expect(ing.quantity, 200.0);
      expect(ing.name, 'flour sifted');
    });

    test('scaling a re-parsed ingredient works end-to-end', () {
      // Simulates a stored plain-text ingredient being re-parsed at display time.
      final stored = Ingredient(name: '200 g flour');
      final parsed = parseIngredientLine(stored.name);
      expect(parsed.toLine(factor: 2.0), '400 g flour');
    });

    test('"of" after unit is skipped', () {
      final ing = parseIngredientLine('1 kg of potatoes');
      expect(ing.quantity, 1.0);
      expect(ing.unit, 'kg');
      expect(ing.name, 'potatoes');
    });

    test('single quantity token with no name returns original text', () {
      // "2" alone — can't infer name
      final ing = parseIngredientLine('2');
      expect(ing.quantity, isNull);
      expect(ing.name, '2');
    });

    test('empty string returns empty name', () {
      final ing = parseIngredientLine('');
      expect(ing.name, '');
      expect(ing.quantity, isNull);
    });

    test('unit-only token is treated as name', () {
      // "g" alone has no leading number, so it's just a name
      final ing = parseIngredientLine('g');
      expect(ing.quantity, isNull);
      expect(ing.name, 'g');
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // ShoppingItem.fromJson  (done field coercion)
  // ──────────────────────────────────────────────────────────────────
  group('ShoppingItem.fromJson', () {
    Map<String, dynamic> base({dynamic done = 0}) => {
          'id': 1,
          'text': '2 apples',
          'done': done,
          'recipe_ids': '[]',
        };

    test('done=0 (int) → false', () {
      expect(ShoppingItem.fromJson(base(done: 0)).done, isFalse);
    });

    test('done=1 (int) → true', () {
      expect(ShoppingItem.fromJson(base(done: 1)).done, isTrue);
    });

    test('done=false (bool) → false', () {
      expect(ShoppingItem.fromJson(base(done: false)).done, isFalse);
    });

    test('done=true (bool) → true', () {
      expect(ShoppingItem.fromJson(base(done: true)).done, isTrue);
    });

    test('done="0" (string) → false', () {
      expect(ShoppingItem.fromJson(base(done: '0')).done, isFalse);
    });

    test('done="1" (string) → true', () {
      expect(ShoppingItem.fromJson(base(done: '1')).done, isTrue);
    });

    test('done=null → false', () {
      expect(ShoppingItem.fromJson(base(done: null)).done, isFalse);
    });

    test('category is preserved when present', () {
      final item = ShoppingItem.fromJson({
        ...base(),
        'category': 'Fruits',
      });
      expect(item.category, 'Fruits');
    });

    test('category is null when absent', () {
      expect(ShoppingItem.fromJson(base()).category, isNull);
    });

    test('recipe_ids JSON array is decoded', () {
      final item = ShoppingItem.fromJson({
        ...base(),
        'recipe_ids': '[1,2,3]',
      });
      expect(item.recipeIds, [1, 2, 3]);
    });

    test('malformed recipe_ids string → empty list, no throw', () {
      final item = ShoppingItem.fromJson({
        ...base(),
        'recipe_ids': 'not-json',
      });
      expect(item.recipeIds, isEmpty);
    });

    test('recipe_titles is preserved', () {
      final item = ShoppingItem.fromJson({
        ...base(),
        'recipe_titles': 'Pasta, Salad',
      });
      expect(item.recipeTitles, 'Pasta, Salad');
    });

    test('notes defaults to empty string when absent', () {
      expect(ShoppingItem.fromJson(base()).notes, '');
    });

    test('notes field is preserved when present', () {
      final item = ShoppingItem.fromJson({
        ...base(),
        'notes': 'whole milk only',
      });
      expect(item.notes, 'whole milk only');
    });

    test('notes=null falls back to empty string', () {
      final item = ShoppingItem.fromJson({
        ...base(),
        'notes': null,
      });
      expect(item.notes, '');
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // splitLines
  // ──────────────────────────────────────────────────────────────────
  group('splitLines', () {
    test('splits on newlines', () {
      expect(splitLines('a\nb\nc'), ['a', 'b', 'c']);
    });

    test('trims whitespace from each line', () {
      expect(splitLines('  a  \n  b  '), ['a', 'b']);
    });

    test('drops empty and blank lines', () {
      expect(splitLines('a\n\n  \nb'), ['a', 'b']);
    });

    test('empty string returns empty list', () {
      expect(splitLines(''), isEmpty);
    });

    test('single non-empty line returns single-element list', () {
      expect(splitLines('hello'), ['hello']);
    });

    test('trailing newline is ignored', () {
      expect(splitLines('a\nb\n'), ['a', 'b']);
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // PrepReminder.fromJson
  // ──────────────────────────────────────────────────────────────────
  group('PrepReminder.fromJson', () {
    test('parses step and hours_before', () {
      final r = PrepReminder.fromJson({'step': 'Marinate', 'hours_before': 24});
      expect(r.step, 'Marinate');
      expect(r.hoursBefore, 24);
    });

    test('float hours_before is truncated to int', () {
      final r = PrepReminder.fromJson({'step': 'Rest dough', 'hours_before': 2.0});
      expect(r.hoursBefore, isA<int>());
      expect(r.hoursBefore, 2);
    });

    test('round-trips via toJson', () {
      const original = PrepReminder(step: 'Soak beans', hoursBefore: 12);
      final roundTripped = PrepReminder.fromJson(original.toJson());
      expect(roundTripped.step, original.step);
      expect(roundTripped.hoursBefore, original.hoursBefore);
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // PrepReminderDto.fromJson
  // ──────────────────────────────────────────────────────────────────
  group('PrepReminderDto.fromJson', () {
    test('parses all fields', () {
      final dto = PrepReminderDto.fromJson({
        'recipe_id': 7,
        'recipe_title': 'Pasta',
        'step': 'Make dough',
        'hours_before': 2,
        'due_date': '2026-03-14',
        'meal_date': '2026-03-15',
      });
      expect(dto.recipeId, 7);
      expect(dto.recipeTitle, 'Pasta');
      expect(dto.step, 'Make dough');
      expect(dto.hoursBefore, 2);
      expect(dto.dueDate, '2026-03-14');
      expect(dto.mealDate, '2026-03-15');
    });

    test('float ids are coerced to int', () {
      final dto = PrepReminderDto.fromJson({
        'recipe_id': 3.0,
        'recipe_title': 'Soup',
        'step': 'Chop',
        'hours_before': 1.0,
        'due_date': '2026-01-01',
        'meal_date': '2026-01-02',
      });
      expect(dto.recipeId, isA<int>());
      expect(dto.hoursBefore, isA<int>());
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // MealPlanEntry.fromJson
  // ──────────────────────────────────────────────────────────────────
  group('MealPlanEntry.fromJson', () {
    test('parses all fields', () {
      final entry = MealPlanEntry.fromJson({
        'id': 7,
        'day': '2026-03-15',
        'recipe_id': 42,
        'title': 'Pasta',
      });
      expect(entry.id, 7);
      expect(entry.day, '2026-03-15');
      expect(entry.recipeId, 42);
      expect(entry.title, 'Pasta');
    });

    test('numeric id is coerced to int', () {
      final entry = MealPlanEntry.fromJson({
        'id': 1.0,
        'day': '2026-01-01',
        'recipe_id': 5.0,
        'title': 'Soup',
      });
      expect(entry.id, isA<int>());
      expect(entry.recipeId, isA<int>());
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // RecipeMacros.fromAny
  // ──────────────────────────────────────────────────────────────────
  group('RecipeMacros.fromAny', () {
    test('reads flat _g keys', () {
      final m = RecipeMacros.fromAny({
        'protein_g': 10.0,
        'fat_g': 5.0,
        'carbs_g': 30.0,
      });
      expect(m.protein, 10.0);
      expect(m.fat, 5.0);
      expect(m.carbs, 30.0);
    });

    test('reads flat non-_g keys', () {
      final m = RecipeMacros.fromAny({
        'protein': 8.0,
        'fat': 3.0,
        'carbs': 20.0,
      });
      expect(m.protein, 8.0);
    });

    test('reads nested macros object', () {
      final m = RecipeMacros.fromAny({
        'macros': {
          'protein_g': 12.0,
          'fat_g': 6.0,
          'carbs_g': 40.0,
        },
      });
      expect(m.protein, 12.0);
      expect(m.fat, 6.0);
      expect(m.carbs, 40.0);
    });

    test('parses per-ingredient list', () {
      final m = RecipeMacros.fromAny({
        'protein_g': 20.0,
        'fat_g': 5.0,
        'carbs_g': 15.0,
        'ingredients': [
          {
            'name': 'chicken',
            'protein_g': 20.0,
            'fat_g': 5.0,
            'carbs_g': 0.0,
            'skipped': false,
          },
        ],
      });
      expect(m.ingredients.length, 1);
      expect(m.ingredients.first.name, 'chicken');
      expect(m.ingredients.first.skipped, isFalse);
    });

    test('empty ingredients list when key absent', () {
      final m = RecipeMacros.fromAny({'protein_g': 1.0, 'fat_g': 1.0, 'carbs_g': 1.0});
      expect(m.ingredients, isEmpty);
    });

    test('integer values are coerced to double', () {
      final m = RecipeMacros.fromAny({'protein_g': 10, 'fat_g': 5, 'carbs_g': 30});
      expect(m.protein, isA<double>());
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // IngredientMacros.fromJson
  // ──────────────────────────────────────────────────────────────────
  group('IngredientMacros.fromJson', () {
    test('parses all fields', () {
      final im = IngredientMacros.fromJson({
        'name': 'butter',
        'protein_g': 0.5,
        'fat_g': 80.0,
        'carbs_g': 0.1,
        'skipped': false,
      });
      expect(im.name, 'butter');
      expect(im.protein, 0.5);
      expect(im.fat, 80.0);
      expect(im.carbs, 0.1);
      expect(im.skipped, isFalse);
    });

    test('skipped defaults to false when absent', () {
      final im = IngredientMacros.fromJson({
        'name': 'oil',
        'protein_g': 0.0,
        'fat_g': 100.0,
        'carbs_g': 0.0,
      });
      expect(im.skipped, isFalse);
    });

    test('skipped=true is preserved', () {
      final im = IngredientMacros.fromJson({
        'name': 'water',
        'protein_g': 0.0,
        'fat_g': 0.0,
        'carbs_g': 0.0,
        'skipped': true,
      });
      expect(im.skipped, isTrue);
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // Recipe.fromJson
  // ──────────────────────────────────────────────────────────────────
  group('Recipe.fromJson', () {
    Map<String, dynamic> minimalRecipe({int id = 1}) => {
          'id': id,
          'title': 'Pasta',
          'source': 'https://example.com',
          'yield': '2 servings',
          'notes': '',
          'created_at': '2026-01-01T00:00:00',
          'updated_at': '2026-01-01T00:00:00',
          'ingredients': [],
          'instructions': [],
        };

    test('parses all base fields', () {
      final r = Recipe.fromJson(minimalRecipe());
      expect(r.id, 1);
      expect(r.title, 'Pasta');
      expect(r.source, 'https://example.com');
      expect(r.yieldText, '2 servings');
      expect(r.notes, '');
      expect(r.ingredients, isEmpty);
      expect(r.instructions, isEmpty);
    });

    test('optional image paths are null when absent', () {
      final r = Recipe.fromJson(minimalRecipe());
      expect(r.imagePathSmall, isNull);
      expect(r.imagePathFull, isNull);
    });

    test('optional image paths are read when present', () {
      final r = Recipe.fromJson({
        ...minimalRecipe(),
        'image_path_small': 'recipes/1/small.webp',
        'image_path_full': 'recipes/1/full.webp',
      });
      expect(r.imagePathSmall, 'recipes/1/small.webp');
      expect(r.imagePathFull, 'recipes/1/full.webp');
    });

    test('shareToken is null when absent', () {
      expect(Recipe.fromJson(minimalRecipe()).shareToken, isNull);
    });

    test('shareToken is read when present', () {
      final r = Recipe.fromJson({...minimalRecipe(), 'share_token': 'abc-123'});
      expect(r.shareToken, 'abc-123');
    });

    test('macros is null when absent', () {
      expect(Recipe.fromJson(minimalRecipe()).macros, isNull);
    });

    test('macros is parsed from nested macros key', () {
      final r = Recipe.fromJson({
        ...minimalRecipe(),
        'macros': {'protein_g': 10.0, 'fat_g': 5.0, 'carbs_g': 20.0},
      });
      expect(r.macros, isNotNull);
      expect(r.macros!.protein, 10.0);
    });

    test('ingredients list is parsed', () {
      final r = Recipe.fromJson({
        ...minimalRecipe(),
        'ingredients': [
          {'name': 'flour', 'quantity': 200.0, 'unit': 'g'},
          {'name': 'salt'},
        ],
      });
      expect(r.ingredients.length, 2);
      expect(r.ingredients[0].name, 'flour');
      expect(r.ingredients[1].unit, isNull);
    });

    test('instructions list is parsed', () {
      final r = Recipe.fromJson({
        ...minimalRecipe(),
        'instructions': ['Mix', 'Bake'],
      });
      expect(r.instructions, ['Mix', 'Bake']);
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // LlmCredits.fromJson
  // ──────────────────────────────────────────────────────────────────
  group('LlmCredits.fromJson', () {
    test('parses all fields', () {
      final c = LlmCredits.fromJson({
        'usage': 0.42,
        'limit': 10.0,
        'is_free_tier': true,
      });
      expect(c.usage, closeTo(0.42, 0.001));
      expect(c.limit, 10.0);
      expect(c.isFreeTier, isTrue);
    });

    test('limit is null when absent', () {
      final c = LlmCredits.fromJson({'usage': 0.1, 'is_free_tier': false});
      expect(c.limit, isNull);
    });

    test('usage defaults to 0 when absent', () {
      final c = LlmCredits.fromJson({'is_free_tier': false});
      expect(c.usage, 0.0);
    });

    test('isFreeTier defaults to false when absent', () {
      final c = LlmCredits.fromJson({'usage': 0.0});
      expect(c.isFreeTier, isFalse);
    });

    test('integer usage is coerced to double', () {
      final c = LlmCredits.fromJson({'usage': 1, 'is_free_tier': false});
      expect(c.usage, isA<double>());
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // mediaUrl helper
  // ──────────────────────────────────────────────────────────────────
  group('mediaUrl', () {
    test('null input → null', () {
      expect(mediaUrl(null), isNull);
    });

    test('empty string → null', () {
      expect(mediaUrl(''), isNull);
    });

    test('relative path gets /media/ prefix', () {
      final url = mediaUrl('recipes/1/small.webp');
      expect(url, endsWith('/media/recipes/1/small.webp'));
    });

    test('path with leading slash is handled correctly', () {
      final url = mediaUrl('/recipes/1/full.webp');
      expect(url, isNotNull);
      expect(url, isNot(contains('/media//')));
    });

    test('cache buster no longer supported → clean URL', () {
      final url = mediaUrl('recipes/1/small.webp');
      expect(url, isNot(contains('?')));
    });

    test('no cache buster → clean URL without query string', () {
      final url = mediaUrl('recipes/1/small.webp');
      expect(url, isNot(contains('?')));
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // MealPlanEntry.fromJson
  // ──────────────────────────────────────────────────────────────────
  group('MealPlanEntry.fromJson', () {
    test('parses all fields', () {
      final e = MealPlanEntry.fromJson({
        'id': 7,
        'day': '2026-03-10',
        'recipe_id': 3,
        'title': 'Pasta',
        'image_path_small': 'recipes/3/small.webp',
      });
      expect(e.id, 7);
      expect(e.day, '2026-03-10');
      expect(e.recipeId, 3);
      expect(e.title, 'Pasta');
      expect(e.imagePathSmall, 'recipes/3/small.webp');
    });

    test('imagePathSmall is null when absent', () {
      final e = MealPlanEntry.fromJson({
        'id': 1,
        'day': '2026-01-01',
        'recipe_id': 1,
        'title': 'Soup',
      });
      expect(e.imagePathSmall, isNull);
    });

    test('numeric id is coerced to int', () {
      final e = MealPlanEntry.fromJson({
        'id': 5.0,
        'day': '2026-01-01',
        'recipe_id': 2.0,
        'title': 'Stew',
      });
      expect(e.id, 5);
      expect(e.recipeId, 2);
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // PrepReminder.fromJson / toJson
  // ──────────────────────────────────────────────────────────────────
  group('PrepReminder', () {
    test('fromJson parses step and hours_before', () {
      final r = PrepReminder.fromJson({'step': 'Soak beans', 'hours_before': 12});
      expect(r.step, 'Soak beans');
      expect(r.hoursBefore, 12);
    });

    test('toJson round-trips correctly', () {
      const r = PrepReminder(step: 'Marinate chicken', hoursBefore: 4);
      final j = r.toJson();
      expect(j['step'], 'Marinate chicken');
      expect(j['hours_before'], 4);
    });

    test('fromJson → toJson round-trip', () {
      final original = {'step': 'Chill overnight', 'hours_before': 8};
      final r = PrepReminder.fromJson(original);
      expect(r.toJson(), original);
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // PrepReminderDto.fromJson
  // ──────────────────────────────────────────────────────────────────
  group('PrepReminderDto.fromJson', () {
    test('parses all fields', () {
      final d = PrepReminderDto.fromJson({
        'recipe_id': 5,
        'recipe_title': 'Lentil Soup',
        'step': 'Soak lentils',
        'hours_before': 6,
        'due_date': '2026-03-09',
        'meal_date': '2026-03-10',
      });
      expect(d.recipeId, 5);
      expect(d.recipeTitle, 'Lentil Soup');
      expect(d.step, 'Soak lentils');
      expect(d.hoursBefore, 6);
      expect(d.dueDate, '2026-03-09');
      expect(d.mealDate, '2026-03-10');
    });
  });

  // ──────────────────────────────────────────────────────────────────
  // ShoppingItem.fromJson
  // ──────────────────────────────────────────────────────────────────
  group('ShoppingItem.fromJson', () {
    test('done=0 (int) → false', () {
      final item = ShoppingItem.fromJson({
        'id': 1,
        'text': 'milk',
        'done': 0,
        'notes': '',
        'recipe_ids': '[]',
      });
      expect(item.done, isFalse);
    });

    test('done=1 (int) → true', () {
      final item = ShoppingItem.fromJson({
        'id': 2,
        'text': 'eggs',
        'done': 1,
        'notes': '',
        'recipe_ids': '[]',
      });
      expect(item.done, isTrue);
    });

    test('done=false (bool) → false', () {
      final item = ShoppingItem.fromJson({
        'id': 3,
        'text': 'butter',
        'done': false,
        'notes': '',
        'recipe_ids': '[]',
      });
      expect(item.done, isFalse);
    });

    test('done=true (bool) → true', () {
      final item = ShoppingItem.fromJson({
        'id': 4,
        'text': 'flour',
        'done': true,
        'notes': '',
        'recipe_ids': '[]',
      });
      expect(item.done, isTrue);
    });

    test('done="1" (string) → true', () {
      final item = ShoppingItem.fromJson({
        'id': 5,
        'text': 'sugar',
        'done': '1',
        'notes': '',
        'recipe_ids': '[]',
      });
      expect(item.done, isTrue);
    });

    test('done="0" (string) → false', () {
      final item = ShoppingItem.fromJson({
        'id': 6,
        'text': 'salt',
        'done': '0',
        'notes': '',
        'recipe_ids': '[]',
      });
      expect(item.done, isFalse);
    });

    test('category is null when absent', () {
      final item = ShoppingItem.fromJson({
        'id': 1, 'text': 'x', 'done': 0, 'notes': '', 'recipe_ids': '[]',
      });
      expect(item.category, isNull);
    });

    test('category is preserved when present', () {
      final item = ShoppingItem.fromJson({
        'id': 1, 'text': 'banana', 'done': 0, 'notes': '', 'recipe_ids': '[]',
        'category': 'Fruits',
      });
      expect(item.category, 'Fruits');
    });

    test('recipe_ids JSON array is parsed', () {
      final item = ShoppingItem.fromJson({
        'id': 1, 'text': 'carrots', 'done': 0, 'notes': '',
        'recipe_ids': '[3, 7, 12]',
      });
      expect(item.recipeIds, [3, 7, 12]);
    });

    test('recipe_ids empty array', () {
      final item = ShoppingItem.fromJson({
        'id': 1, 'text': 'onion', 'done': 0, 'notes': '',
        'recipe_ids': '[]',
      });
      expect(item.recipeIds, isEmpty);
    });

    test('recipe_ids malformed JSON falls back to empty list', () {
      final item = ShoppingItem.fromJson({
        'id': 1, 'text': 'garlic', 'done': 0, 'notes': '',
        'recipe_ids': 'not-json',
      });
      expect(item.recipeIds, isEmpty);
    });

    test('notes defaults to empty string when null', () {
      final item = ShoppingItem.fromJson({
        'id': 1, 'text': 'tomato', 'done': 0, 'recipe_ids': '[]',
      });
      expect(item.notes, '');
    });

    test('recipe_titles is read when present', () {
      final item = ShoppingItem.fromJson({
        'id': 1, 'text': 'pasta', 'done': 0, 'notes': '', 'recipe_ids': '[1]',
        'recipe_titles': 'Carbonara, Bolognese',
      });
      expect(item.recipeTitles, 'Carbonara, Bolognese');
    });
  });
}
