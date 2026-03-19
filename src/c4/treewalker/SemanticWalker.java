package treewalker;

/*
 * Dies ist eine Unterklasse der Klasse TreeWalker, die den Syntaxbaum 
 * durchläuft und eine Typprüfung durchführt.
 * Es werden automatische Typanpassungen durchgeführt und 
 * entsprechende Knoten in den Syntaxbaum eingeführt
 *
 * @author rp
 */


import lexer.Tag;
import lexer.Word;
import lexer.Array;
import lexer.Type;
import inter.*;

public class SemanticWalker extends TreeWalker<Type, Void>{
	

	void error(int line, String s) {
		throw new Error("Type error near line: " + line + "  " + s);
	}
	
	boolean numeric(Type p) {
		return (p == Type.Char || p == Type.Int || p == Type.Float);
	}

	/*
	 * max gibt den Ergebnistyp bei automatischer Typanpassung zurück
	 */
	
	Type maxType(Type p1, Type p2) {
		if (!numeric(p1) || !numeric(p2))
			return null;
		else if (p1 == Type.Float || p2 == Type.Float)
			return Type.Float;
		else if (p1 == Type.Int || p2 == Type.Int)
			return Type.Int;
		else 
			return Type.Char;
	}

		/*
		 * coerceExpr führt eine Typanpassung des Ausdrucks e auf den 
		 * Zieltyp t durch Einfügen der passenden unären Operation aus
		 */
	
	Expr coerceExpr(Expr e, Type t) {
		Word conv = (t == Type.Int) ? Word.toInt : Word.toFloat;
		Unary node = new Unary(conv, e);
		node.setType(t);
		return node;
	}
	

	boolean checkLogical(Type p1, Type p2) {
		return (p1 == Type.Bool) && (p2 == Type.Bool);
	}
	
	/*
	 * checkArrayAcc prüft, ob das erste Argument ein Array ist und ob das
	 * zweite Argument vom Typ Integer oder Char ist. Ist der Test 
	 * erfolgreich, wird der Typ eine ArrayEintrags zurückgegeben.
	 * Andernfalls ist die Rückgabe null
	 */
	
	Type checkArrayAcc (Type a, Type t) {
		if (a.tag == Tag.INDEX && (t == Type.Int || t == Type.Char))
			return ((Array)a).getOf();
		else
			return null;
	}
	

	/*
	 * ----------------------------------------
     *  ab hier beginnen die walk-Methoden  
     * ----------------------------------------
	 */
	
	@Override
	public Type walkAccessNode(Access node, Void arg) {
		Type ind = walk(node.getIndex(), null);
		Type a = walk(node.getArray(), null);	
		
		Type resType = checkArrayAcc(a, ind);
		if (resType == null)
			error(node.getLexline(), "array type error");
	
		node.setType(resType);
		
		// falls mit einem Character indiziert wird, muss der zu einem
		// Integer gewandelt werden. 
		
		if (ind == Type.Char){
			node.setIndex(coerceExpr(node.getIndex(), Type.Int));
		}
		return resType;
	}

	@Override
	public Type walkAndNode(And node, Void arg) {
		Type p1 = walk(node.getExpr1(), null);
		Type p2 = walk(node.getExpr2(), null);
		
		if (!checkLogical(p1,p2))
			error (node.getLexline(), "node: and");
		node.setType(Type.Bool);
		return Type.Bool;
	}


	@Override
	public Type walkArithNode(Arith node, Void arg) {
		Type p1 = walk(node.getExpr1(), null);
		Type p2 = walk(node.getExpr2(), null);
		Type resType = maxType(p1, p2);
		
		if (resType == null)
			error (node.getLexline(), "node: Arith");
		if (p1 != resType) {
			node.setExpr1(coerceExpr(node.getExpr1(), resType));
		}
		else if (p2 != resType) {
			node.setExpr2 (coerceExpr(node.getExpr2(), resType));
		}
		node.setType(resType);
		return resType;
	}

	@Override
	public Type walkAssignElemNode(AssignElem node, Void arg) {
		Type p1 = walk(node.getAcc(), null);
		Type p2 = walk(node.getExpr(), null);
		Type resType = maxType(p1, p2);
		
		if(p1 == p2 && numeric(p1)) {
			return null;			
		}
		if(checkLogical(p1,p2)){
			return null;
		}
		if (numeric(p1)&& (p1 == resType)){
			node.setExpr(coerceExpr(node.getExpr(), p1));
		} else{
			error(node.getLexline(), "incompatible array assignment");
		}
		return null;		
	}

	@Override
	public Type walkAssignIdNode(AssignId node, Void arg) {
		Type p1 = walk(node.getIdent(), null);
		Type p2 = walk(node.getExpr(), null);
		Type resType = maxType(p1, p2);
		
		if(p1 == p2 && numeric(p1)) {
			return null;			
		}
		if(checkLogical(p1,p2)){
			return null;
		}
		if (numeric(p1)&& (p1 == resType)){
			node.setExpr(coerceExpr(node.getExpr(), p1));
		} else{
			error(node.getLexline(), "incompatible assignment");
		}
		return null;		
	}
	
	@Override
	public Type walkAssignStmtNode(AssignStmt node, Void arg) {
		return walk(node.getAssign(), null);
	}

	@Override
	public Type walkBlockNode(Block node, Void arg) {
		walk(node.getStmts(), null);
		return null;

	}

	@Override
	public Type walkBreakNode(Break node, Void arg) {
		if (Stmt.getEnclosing() == null)
				error(node.getLexline(), "unenclosed break");
		// fuer die spaetere Codererzeugung:
		node.setStmt(Stmt.getEnclosing());
		return null;
	}

	@Override
	public Type walkConstantNode(Constant node, Void arg) {
		return node.getType();
	}

	@Override
	public Type walkDoNode(Do node, Void arg) {
		if (walk(node.getExpr(), null) != Type.Bool) {
			error(node.getLexline(), "Boolean required in do");
		}
		Stmt savedStmt = Stmt.getEnclosing();
		Stmt.setEnclosing(node);
		walk(node.getStmt(), null);
		Stmt.setEnclosing(savedStmt);
		return null;
	}

	@Override
	public Type walkElseNode(Else node, Void arg) {
		if (walk(node.getExpr(), null) != Type.Bool) {
			error(node.getLexline(), "Boolean required in if");
		}
		walk(node.getStmt1(), null);
		walk(node.getStmt2(), null);
		return null;
	}

	@Override
	public Type walkEmptyStmtNode(EmptyStmt node, Void arg) {
		return null;
	}

	@Override
	public Type walkForNode(For node, Void arg) {
		Stmt savedStmt = Stmt.getEnclosing();
		Stmt.setEnclosing(node);
		walk(node.getInit_ass(), null);
		if (walk(node.getExpr(), null) != Type.Bool) {
			error(node.getLexline(), "Boolean required in for");
		}
		walk(node.getIter_ass(), null);		
		walk(node.getStmt(), null);
		Stmt.setEnclosing(savedStmt);
		return null;
	}

	@Override
	public Type walkIdNode(Id node, Void arg) {
		return node.getType();
	}

	@Override
	public Type walkIfNode(If node, Void arg) {
		if (walk(node.getExpr(), null) != Type.Bool) {
			error(node.getLexline(), "Boolean required in if");
		}
		walk(node.getStmt(), null);
		return null;
	}

	@Override
	public Type walkNotNode(Not node, Void arg) {
		if (walk(node.getExpr1(), null) != Type.Bool)
			error(node.getLexline(), "node: Not");
		node.setType(Type.Bool);
		return Type.Bool;
	}

	@Override
	public Type walkOrNode(Or node, Void arg) {
		Type p1 = walk(node.getExpr1(), null);
		Type p2 = walk(node.getExpr2(), null);
		
		if (!checkLogical(p1,p2))
			error (node.getLexline(), "node: or");
		node.setType(Type.Bool);
		return Type.Bool;
	}

	@Override
	public Type walkProgramNode(Program node, Void arg) {
		walk(node.getBlock(), null);
		return null;
	}

	@Override
	public Type walkRelNode(Rel node, Void arg) {
		Type p1 = walk(node.getExpr1(), null);
		Type p2 = walk(node.getExpr2(), null);
		Type resType = maxType(p1,p2);
		
		if (resType == null)
			error (node.getLexline(), "node: Rel" );
		if (p1 != resType) {
			node.setExpr1(coerceExpr(node.getExpr1(), resType));
		}
		else if (p2 != resType) {
			node.setExpr2(coerceExpr(node.getExpr2(), resType));
		}
		node.setType(Type.Bool);
		return Type.Bool;
	}

	@Override
	public Type walkSeqNode(Seq node, Void arg) {
		walk(node.getStmt1(), null);
		walk(node.getStmt2(), null);
		return null;
	}
	
	@Override
	public Type walkUnaryNode(Unary node, Void arg) {
		Type p = walk(node.getExpr(), null);
		if ( !numeric(p))
			error (node.getLexline(), "node: Unary");
		node.setType(p);
		return p;
	}
	
	@Override
	public Type walkWhileNode(While node, Void arg) {
		if (walk(node.getExpr(), null) != Type.Bool) {
			error(node.getLexline(), "Boolean required in while");
		}
		Stmt savedStmt = Stmt.getEnclosing();
		Stmt.setEnclosing(node);
		walk(node.getStmt(), null);
		Stmt.setEnclosing(savedStmt);
		return null;
	}

}
