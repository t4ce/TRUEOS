package treewalker;


import java.util.*;
import treewalker.TreeWalker;
import inter.*;
import code.*;
import main.Labels;
import lexer.Token;
import lexer.Word;
import lexer.Type;

/*
 * Dies ist eine Unterklasse der Klasse TreeWalkerC, die den Syntaxbaum 
 * durchläuft und expliziten Drei-Adress Code erzeugt. 
 * 
 * Zur Vermeidung von zu vielen temporären Variablen geben die walk-Methoden 
 * der Unterklassen von Expr einen singulären Syntaxbaum-Knoten zurück, der mit der
 * toString-Methode zum linken oder rechten Teil eines 3-Adress-Befehls transformiert
 * werden kann. 
 *
 * Die erzeugten Drei-Adress-Befehle werden in eine ArrayListe code geschrieben.
 * Sprungziele sind die Indizes der Befehle. Um Vorwärtssprünge zu realisieren
 * wird ein Mapping Labelnummer -> Index aufgebaut und die Sprungziele Ende der 
 * Code-Erzeugung entsprechend substituiert.
 * 
 */



public class CodeGenWalker extends TreeWalker<Expr, Labels> {
	
	// Das code Array für das erzeugte Drei-Adress-Befehle
	// nextCode gibt den nächsten freien Platz im Array code an
	// zur Anpassung der Konvention im Drachenbuch beginnt das code
	// Array bei Index 1.
	
	private int nextCode;
	public  ArrayList<DreiAdrCode> code;
	private int labels; // zur fortlaufenden Nummerierung der Labels
	
	public CodeGenWalker() {
		Temp.resetCounter();  // Zähler für temporäre Variablen zurücksetzen
		nextCode = 1;		  // der erste Befehl steht auf Platz 1 
		labels = 0;
		code = new ArrayList<DreiAdrCode>();
	}

	// Für die Umsetzung der Labels zu Positionen im code-Array
	private Hashtable<Integer, Integer> labelIsOnPosition = new Hashtable<>();	
	
	// nächstes Label erzeugen
	int newLabel() {
		return ++labels;
	}
	
	/*
	 * virtuelle Ausgabe eines Labels
	 * Es wird im entsprechenden Eintrag des LabelArrays die Position
	 * des nächten freien Platzes im code-Array festgehalten.
	 */

	void emitLabel(int label) {
		labelIsOnPosition.put(label, nextCode);	
	}
	

	void emitCode(DreiAdrCode c) {
		code.add(c);
		nextCode++;
	}
	/*
	 * Diese Funktion wird am Ende aufgerufen, um die in den Sprungbefehlen vorhandenen 
	 * Labels durch Positionen im code-Array zu ersetzen.
	 */
	void adjustLabels() {
		int k;
		for (DreiAdrCode c : code) {
			k = c.getLabel();
			if (k != 0)
				c.setLabel(labelIsOnPosition.get(k));
		}
	}
	
	/*
	 * Diese Methode wählt je nach Klasse von exp den passenden bedingten Sprungbefehl
	 */
	void chooseJumps(Expr exp, int l) {
		if (exp.isSingleton()) {
			emitCode(new BedJumpCodeId((Singleton)exp, l));
		}
		else {
			emitCode(new BedJumpCodeRel((Logical)exp, l));
		}
	}
	
	/*
	 * Da hier im Gegensatz zum Buch kein Sprung-Befehl der Form 
	 * iffalse ... verwendet werden soll, wird dieser Befehl durch 
	 * Austausch des relationen Operators im Ausdruck exp simuliert
	 */
	Logical changeBooleanValue(Expr exp) {
		Token  t  = exp.getOp();	// relationaler Operator
		Token tneu = null;
		switch (t.tag) {
		case LESS : tneu = Word.ge;
			break;
		case GREATER: tneu = Word.le;
			break;
		case GE : tneu =Word.ls;
			break;
		case LE : tneu = Word.gr;
			break;
		case EQ : tneu = Word.ne;
			break;
		case NEQ : tneu = Word.eq;
			break;
		default: ;
			System.out.println("Error: ChangeBoolean Value");
		}
		exp.setOp(tneu);
		return ((Logical)exp);
	}
	
	
	/*
	 * Boolesche Ausdrücke werden mit der short-cut Methode 
	 * ausgewertet. Die in arg übergebenen Label werden
	 * der Booleschen Funktion entsprechend weitergereicht.
	 * Das Label 0 steht dabei immer für die Fortsetzung des 
	 * Programmlaufs ohne Sprung.
	 */
	
	void emitGotos(Expr exp, Labels args) {
		int t = args.trueLabel();
		int f = args.falseLabel();
		if (t != 0 && f != 0) {
			chooseJumps(exp, t);
			emitCode(new UnbJumpCode(f));
		}
		else if (t != 0) {
			chooseJumps(exp, t);
		}
		else if (f != 0) {
			
			// Sonderfall, falls exp ein Singleton ist.
			// hier funktioniert die Simulation von iffalse durch Austasch 
			// des relationalen Operators ja nicht.
			
			if (exp.isSingleton()) {
				int label = newLabel();
				emitCode(new BedJumpCodeId((Singleton)exp,label));
				emitCode(new UnbJumpCode(f));
				emitLabel(label);
			}
			else
				emitCode(new BedJumpCodeRel(changeBooleanValue(exp), f));
		}
		else;
	}

	/*
	 * Wenn der Ausdruck vom Typ Boolean ist, muss er speziell
	 * verarbeitet werden, da in unserem 3-Adress-Befehlen
	 * keine Booleschen Operationen erlaubt sind.
	 * Eine neue temporäre Variable wird einmal auf true und einmal
	 * auf false gesetzt und entsprechende Sprünge erzeugt
	 */
	
	Expr processBooleanExpr(Expr node) {
		if (node.isConstant()) {
			return (Singleton)node;
		}
		
		int t = newLabel();
		int f = newLabel();
		if(node.isConstant()) return node;
		Temp tmp = new Temp(Type.Bool);
		walk(node, new Labels(0,f));
		emitCode(new AssignCode(tmp, Constant.True));
		emitCode(new UnbJumpCode(t));
		emitLabel (f);
		emitCode(new AssignCode(tmp, Constant.False));
		emitLabel(t);
		return tmp;		
	}
	
	/*
	 * reduce gibt den Namen einer Variablen zurück, die nach Auswertung
	 * den Wert von exp enthält. Ist exp keine Konstante oder Variable, 
	 * dann entscheidet die Klasse des Knotens, welcher Befehl erzeugt wird.
	 */

	Singleton reduce (Expr exp) {
		if (exp.isSingleton())
			return (Singleton) exp;
		Temp t = new Temp(exp.getType());
		emitCode(exp.codeForValueTo(t));
		return t;
	}

	/*
	 * ----------------------------------------
     *  ab hier beginnen die walk-Methoden  
     * ----------------------------------------
     * Jede Methode für einen Ausdruck bekommt das Paar (trueLabel, falseLabel) als Argument
     *  und liefert ein Singleton als Wert des Attributs place zurück.
	 */



	@Override
	public Expr walkAccessNode(Access node, Labels arg) {
		Expr e = walk (node.getIndex(), arg);	
		Access a = new Access(node.getArray(), reduce(e), node.getType());
		if (node.getType() == Type.Bool) {
			Expr t = reduce(a);
			emitGotos(t,arg);
			return t; 
		}
		else {	
			return a;
		}
	}

	/*
	 * Boolesche Ausdrücke werden mit der short-cut Methode 
	 * ausgewertet. Die in arg übergebenen Label werden
	 * der Booleschen Funktion entsprechend weitergereicht.
	 * Das Label 0 steht dabei immer für die Fortsetzung des 
	 * Programmlaufs ohne Sprung.
	 */


	@Override
	public Expr walkAndNode(And node, Labels arg) {
		int t = arg.trueLabel();
		int f = arg.falseLabel();
		
		int label = f != 0 ? f : newLabel();
		walk (node.getExpr1(), new Labels(0,label));
		walk (node.getExpr2(), new Labels(t,f));
		if (f == 0)
			emitLabel(label);
		return null;
	}

	@Override
	public Expr walkArithNode(Arith node, Labels arg) {
		Expr e1 = walk(node.getExpr1(), arg);
		Expr e2 = walk(node.getExpr2(), arg);		
		return new Arith(node.getOp(), reduce(e1), reduce(e2));
	} 
	
	/*
	 * wenn das Array vom Typ Bool ist,
	 * müssen explizite Boolesche Werte gespeichert werden. 
	 */

	@Override
	public Expr walkAssignElemNode(AssignElem node, Labels arg) {
		Expr e1;
		if (node.getExpr().getType() == Type.Bool) 
			e1 = processBooleanExpr(node.getExpr());	
		else
			e1 = reduce(walk(node.getExpr(), arg));
		Access acc = node.getAcc();
		Expr ind = reduce(walk(acc.getIndex(), null));
		emitCode(new ArrayAssignCode(new Access(acc.getArray(), ind), (Singleton)e1));
		return null;
	}
	/*
	 * wenn der Identifier eine boolesche Variable ist,
	 * müssen explizite Boolesche Werte gespeichert werden. 
	 */

	@Override
	public Expr walkAssignIdNode(AssignId node, Labels arg) {
		Expr eCode;
		if (node.getExpr().getType() == Type.Bool) {
			eCode = processBooleanExpr(node.getExpr());	
		} else {
			eCode = walk(node.getExpr(), arg);
		}
		emitCode(eCode.codeForValueTo(node.getIdent()));
		return null;
	}

	@Override
	public Expr walkAssignStmtNode(AssignStmt node, Labels arg) {
		walk(node.getAssign(), arg);
		return null;
	}

	@Override
	public Expr walkBlockNode(Block node, Labels arg) {
		walk(node.getStmts(), arg);
		return null;
	}

	@Override
	public Expr walkBreakNode(Break node, Labels arg) {
		emitCode(new UnbJumpCode(node.getStmt().getNext()));
		return null;
	}

	@Override
	public Expr walkConstantNode(Constant node, Labels arg) {
		if (node == Constant.True && arg.trueLabel() != 0) {
			emitCode(new UnbJumpCode(arg.trueLabel()));
		}
		if (node == Constant.False && arg.falseLabel() != 0) {
			emitCode(new UnbJumpCode(arg.falseLabel()));
		}
		return node;
	}

	@Override
	public Expr walkDoNode(Do node, Labels arg) {
		int label = newLabel();	
		int beginLabel = newLabel();

		node.setNext(arg.nextLabel());	// fur das break-Statement
		emitLabel(beginLabel);
		walk(node.getStmt(), new Labels(label));
		emitLabel(label);
		walk(node.getExpr(), new Labels(beginLabel,0));
		return null;		
	}

	@Override
	public Expr walkElseNode(Else node, Labels arg) {
		int label = newLabel();
		int next = arg.nextLabel();
		
		walk(node.getExpr(), new Labels (0,label));
		walk (node.getStmt1(), arg);
		emitCode(new UnbJumpCode(next));
//		emit("goto L" + next);
		emitLabel(label);
		walk(node.getStmt2(), arg);
		return null;
	}

	@Override
	public Expr walkEmptyStmtNode(EmptyStmt node, Labels arg) {
		return null;
	}

	@Override
	public Expr walkForNode(For node, Labels arg) {
		int label1 = newLabel();
		int label2 = newLabel();
		int next = arg.nextLabel();
		
		node.setNext(next);
		walk(node.getInit_ass(), arg);
		emitLabel(label1);
		walk(node.getExpr(), new Labels(0, next));
		walk(node.getStmt(), new Labels(label2));
		emitLabel(label2);
		walk(node.getIter_ass(), arg);
		emitCode(new UnbJumpCode(label1));
		return null;
	}

	@Override
	public Expr walkIdNode(Id node, Labels arg) {
		if (node.getType() == Type.Bool)
			emitGotos(node, arg);
		return node;
	}

	@Override
	public Expr walkIfNode(If node, Labels arg) {
		int next = arg.nextLabel();
		
		walk(node.getExpr(), new Labels (0,next));
		walk (node.getStmt(), new Labels(next));		
		return null;
	}

	@Override
	public Expr walkNotNode(Not node, Labels arg) {
		walk(node.getExpr1(), new Labels(arg.falseLabel(), arg.trueLabel()));
		return null;
	}

	@Override
	public Expr walkOrNode(Or node, Labels arg) {
		int t = arg.trueLabel();
		int f = arg.falseLabel();
		
		int label = t != 0 ? t : newLabel();
		walk (node.getExpr1(), new Labels(label,0));
		walk (node.getExpr2(), new Labels(t,f));
		if (t == 0)
			emitLabel(label);
		return null;
	}

	@Override
	public Expr walkProgramNode(Program node, Labels arg) {
		int end = newLabel();
		
		walk(node.getBlock(), new Labels(end));
		emitLabel(end);
		// Justierung der Sprungbefehle 
		adjustLabels();
		return null;
	}

	@Override
	public Expr walkRelNode(Rel node, Labels arg) {
		Expr e1 = reduce(walk (node.getExpr1(), null));
		Expr e2 = reduce(walk (node.getExpr2(), null));
		emitGotos(new Rel(node.getOp(), e1, e2), arg);
		return null;
	}

	@Override
	public Expr walkSeqNode(Seq node, Labels arg) {
		if (node.getStmt1() == EmptyStmt.Null) 
			walk(node.getStmt2(), arg);
		else if (node.getStmt2() == EmptyStmt.Null)
			walk(node.getStmt1(), arg);
		else {
			int label = newLabel();
			walk(node.getStmt1(), new Labels(label));
			emitLabel(label);
			walk(node.getStmt2(), arg);
		}
		return null;
	}

	@Override
	public Expr walkUnaryNode(Unary node, Labels arg) {
		Expr e = walk(node.getExpr(), arg);
		return new Unary(node.getOp(), reduce(e));
	}

	@Override
	public Expr walkWhileNode(While node, Labels arg) {
		
		int next = arg.nextLabel();
		int beginLabel = newLabel();
		
		node.setNext(next);		// fur das break-Statement
		emitLabel(beginLabel);
		walk(node.getExpr(), new Labels(0,next));
		walk(node.getStmt(), new Labels(beginLabel));
		emitCode(new UnbJumpCode(beginLabel));
//		emit("goto L" + beginLabel);
		return null;
		
	}


}
