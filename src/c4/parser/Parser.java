package parser;

import java.io.*;
import lexer.*;
import inter.*;
import main.Env;


/*
 * Diese Klasse implementiert einen recursive descent Parser für die 
 * Beispielsprache. Die Instanzenvariable lex verweist auf einen lexikalen
 * Scanner für diese Sprache. look enthält das lookahead-Token 
 * Bei Erkennen einer syntaktischen Struktur wird ein entsprechender
 * Knoten des Syntaxbaums erzeugt und verknüpft.
 */

public class Parser {
	private Lexer lex; 	// Lexical Analyser für diesen Parser
	private Token look; // lookahead Token
	Env top = null;		// die momentan aktuelle Symboltabelle ist leer
	int used = 0;		// nächste freie Speicheradresse im Datenspeicher

	public Parser(Lexer l) throws IOException {
		lex = l;
		move();
	}

	/* move liest das nächste Token und speichert es in look */
	private void move() throws IOException {
		look = lex.scan();
	}

	private void error(String s) {
		throw new Error("near line " + Lexer.line + ": " + s);
	}

	private void match(Tag t) throws IOException {
		if (look.tag == t)
			move();
		else
			error("syntax error, expected '" + t.lexeme() + "'");
	}

	public Program program() throws IOException {
		return new Program(block());
	}

	private Block block() throws IOException {
		int savedUsed = used;
		match(Tag.LBRACE);
		Env savedEnv = top;								// momentane Symboltabelle retten
		top = new Env(top);								// neue leere Symboltabelle mit der alten verknüpfen
		decls();
		Stmt statements = stmts();
		match(Tag.RBRACE);
		top = savedEnv;									// Symboltabelle auf den Stand vor dem Block zurücksetzen 
		used = savedUsed;
		return new Block(statements);
	}

	private void decls() throws IOException {
		Token tok;
		while (look.tag == Tag.BASIC) {
			Type p = type();
			do {
				tok = look;
				match(Tag.ID);
				Id id = new Id((Word)tok, p, used);
				if (top.put(tok,  id) != null) {	// Eintragen in Symboltabelle - falls tok
													// schon vorhanden, gibt put Wert != null zurück
					error("Variable " + id.getOp().toString() + " redeclared");	
				}	
				used += p.getWidth();

				if (look.tag != Tag.COMMA) 	// kein weiterer ID zu dieser Typ-Deklaration
					break;
				move();					// Komma überlesen
			} while (true);

			match(Tag.SEMI);
		}
	}

	private Type type() throws IOException {
		// expect look.tag == Tag.Basic
		Type p = (Type) look;
		match(Tag.BASIC);
		if (look.tag != Tag.LBRACKET) 
			return p;		// Type -> basic					// dims -> epsilon
		else
			return dims(p);
	}

	private Type dims(Type p) throws IOException {				// dims -> [num] dims
		//  Der Grundtyp des Feldes wird als Parameter übergeben
			match(Tag.LBRACKET);
			Token tok = look;
			match(Tag.NUM);
			match(Tag.RBRACKET);
			if (look.tag == Tag.LBRACKET) 
				p = dims(p);
			return new Array(((Num)tok).value, p);
	}

	private Stmt stmts() throws IOException {
		if (look.tag == Tag.RBRACE) {
			return EmptyStmt.Null;
		} else {
			Stmt statement = stmt();
			Stmt nextStatement = stmts();
			return new Seq(statement, nextStatement);
		}
	}

	private Stmt stmt() throws IOException {
		Stmt statement1, statement2;
		Expr expression;
		Assignment assignment1, assignment2;

		switch (look.tag) {
		case SEMI:
			move();
			return EmptyStmt.Null;

		case IF:
			move();
			match(Tag.LPARA);
			expression = bool();
			match(Tag.RPARA);
			statement1 = stmt();

			if (look.tag == Tag.ELSE) {
				move();
				statement2 = stmt();
				return new Else(expression, statement1, statement2);
			}

			return new If(expression, statement1);

		case WHILE:
			move();
			match(Tag.LPARA);
			expression = bool();
			match(Tag.RPARA);
			statement1 = stmt();
			return new While(expression, statement1);

		case DO:
			move();
			statement1 = stmt();
			match(Tag.WHILE);
			match(Tag.LPARA);
			expression = bool();
			match(Tag.RPARA);
			match(Tag.SEMI);
			return new Do(statement1, expression);

		case BREAK:
			move();
			match(Tag.SEMI);
			return new Break();

		case LBRACE:
			return block();

		case ID:
			assignment1 = assign();
			match(Tag.SEMI);
			return new AssignStmt(assignment1);

		// stmt -> for ( assign ; bool ; assign) stmt
		case FOR:
			move();
			match(Tag.LPARA);
			assignment1 = assign();
			match(Tag.SEMI);
			expression = bool();
			match(Tag.SEMI);
			assignment2 = assign();
			match(Tag.RPARA);
			statement1 = stmt();
			return new For(assignment1, expression, assignment2, statement1);

		default:
			error("expected statement");
			return null;
		}
	}

	private Assignment assign() throws IOException {
		Token idToken = look;
		match(Tag.ID);
		Id id = top.get(idToken);
		if (id == null)
			error(idToken.toString() + " undeclared"); 					
		Expr expression;
		if (look.tag == Tag.EQS) {
			move();
			expression = bool();
			return new AssignId(id, expression);
		} else {
			Access access = offset(id);
			match(Tag.EQS);
			expression = bool();
			return new AssignElem(access, expression);
		}
	}

	private Expr bool() throws IOException {
		Expr expression1 = join();
		while (look.tag == Tag.OR) {
			Token orToken = look;
			move();
			Expr expression2 = join();
			expression1 = new Or(orToken, expression1, expression2);
		}
		return expression1;
	}

	private Expr join() throws IOException {
		Expr expression1 = equality();
		while (look.tag == Tag.AND) {
			Token andToken = look;
			move();
			Expr expression2 = equality();
			expression1 = new And(andToken, expression1, expression2);
		}
		return expression1;
	}

	private Expr equality() throws IOException {
		Expr expression1 = rel();
		while (look.tag == Tag.EQ || look.tag == Tag.NEQ) {
			Token eqToken = look;
			move();
			Expr expression2 = rel();
			expression1 = new Rel(eqToken, expression1, expression2);
		}
		return expression1;
	}

	private Expr rel() throws IOException {
		Expr expression1 = expr();
		switch (look.tag) {
		case LESS:
		case LE:
		case GE:
		case GREATER:
			Token relToken = look;
			move();
			Expr expression2 = expr();
			return new Rel(relToken, expression1, expression2);
		default:
			return expression1;
		}
	}

	private Expr expr() throws IOException {
		Expr expression1 = term();
		while (look.tag == Tag.PLUS || look.tag == Tag.MINUS) {
			Token arithToken = look;
			move();
			Expr expression2 = term();
			expression1 = new Arith(arithToken, expression1, expression2);
		}
		return expression1;
	}

	private Expr term() throws IOException {
		Expr expression1 = unary();
		while (look.tag == Tag.MUL || look.tag == Tag.DIV) {
			Token arithToken = look;
			move();
			Expr expression2 = unary();
			expression1 = new Arith(arithToken, expression1, expression2);
		}
		return expression1;
	}

	private Expr unary() throws IOException {
		if (look.tag == Tag.MINUS) {
			move();
			Expr expression = unary();
			return new Unary(new Token(Tag.UMINUS), expression);
		} else if (look.tag == Tag.NOT) {
			Token uToken = look;
			move();
			Expr expression = unary();
			return new Not(uToken, expression);
		} else {
			return factor();
		}

	}

	private Expr factor() throws IOException {
		Token token;
		Expr expression;

		switch (look.tag) {
		case LPARA:
			move();
			expression = bool();
			match(Tag.RPARA);
			return expression;
		case NUM:
			token = look;
			move();
			return new Constant(token, Type.Int);
		case REAL:
			token = look;
			move();
			return new Constant(token, Type.Float);
		case TRUE:
			move();
			return Constant.True;
		case FALSE:
			move();
			return Constant.False;
		case ID:
			Id id = top.get(look);
			if (id == null)
				error(look.toString() + " undeclared");
				
			move();
			if (look.tag != Tag.LBRACKET) {
				return id;
			}
			else {
				return offset(id);
			}

		default:
			error("expected factor");
			return null;
		}
	}

	private Access offset(Id id) throws IOException {
		move();
		Expr expression = bool();
		match(Tag.RBRACKET);

		Access access = new Access(id, expression);

		while (look.tag == Tag.LBRACKET) {
			move();
			expression = bool();
			match(Tag.RBRACKET);
			access = new Access(access, expression);
		}
		return access;
	}

}
