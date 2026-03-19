package code;


public abstract class DreiAdrCode {

	// getLabel liefert nur bei Befehlen der Unterklasse JumpCode den momentanen Label zurück
	// bei allen anderen Befehlsformen wird 0 zurückgegegeben
	public int getLabel()
	{
		return 0;  // Goto-Befehl!
	}

	public void setLabel(int i)
	{
		System.out.println("setLabel auf einen Nicht-Sprungbefehl!");
	}


}