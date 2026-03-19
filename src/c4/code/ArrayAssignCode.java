                                         package code;
import inter.Singleton;
import inter.Id;
import inter.Access;

/*
 * Diese Klasse definiert Drei-Adress-Befehle der Form array[index] = rightSide
 */


public class ArrayAssignCode extends ArrayCode {
	Singleton rightSide;

	public ArrayAssignCode(Access acc, Singleton r) {
		array = (Id) acc.getArray();
		index = (Singleton)acc.getIndex();
		rightSide = r;
	}
	
	public String toString()
	{
		return (array.toString() + "[" + index.toString() + "] = " + rightSide.toString());
	}
}
